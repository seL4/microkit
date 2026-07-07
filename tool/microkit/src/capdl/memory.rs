//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//
use crate::{
    capdl::{
        spec::capdl_obj_human_name, util::capdl_util_make_cte, CapDLNamedObject,
        CapDLSpecContainer, FrameFill,
    },
    sdf::IommuDeviceIdentifier,
    sel4::{Arch, Config, ObjectType, PageSize},
};
use sel4_capdl_initializer_types::{
    cap, object, x86_io_address_space, Cap, Object, ObjectId, Word,
};
use std::ops::Range;

/// For naming and debugging purposes only, no functional purpose.
fn get_pt_level_name(sel4_config: &Config, level: usize) -> &str {
    match sel4_config.arch {
        crate::sel4::Arch::Aarch64 => match level {
            0 => "pgd",
            1 => "pud",
            2 => "pd",
            3 => "pt",
            _ => unreachable!(
                "get_pt_level_name(): internal bug: unknown page table level {} for aarch64",
                level
            ),
        },
        crate::sel4::Arch::Riscv64 => match level {
            0 => "pgd",
            1 => "pmd",
            2 => "pte",
            _ => unreachable!(
                "get_pt_level_name(): internal bug: unknown page table level {} for riscv64",
                level
            ),
        },
        crate::sel4::Arch::X86_64 => match level {
            0 => "pml4",
            1 => "pdpt",
            2 => "pd",
            3 => "pt",
            _ => unreachable!(
                "get_pt_level_name(): internal bug: unknown page table level {} for x86_64",
                level
            ),
        },
    }
}

fn get_iopt_level_name(level: usize) -> String {
    if level == 0 {
        "iospace".to_string()
    } else {
        format!("iopt_level_{}", level - 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressSpace {
    VSpace {
        name: String,
        root: ObjectId,
        x86_ept: bool,
    },
    IOSpace {
        name: String,
        root: ObjectId,
        device: IommuDeviceIdentifier,
    },
}

impl AddressSpace {
    pub fn root(&self) -> ObjectId {
        match self {
            &Self::VSpace { root, .. } | &Self::IOSpace { root, .. } => root,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::VSpace { name, .. } | Self::IOSpace { name, .. } => name,
        }
    }

    pub fn map_page(
        &self,
        spec_container: &mut CapDLSpecContainer,
        sel4_config: &Config,
        frame_cap: Cap,
        frame_size_bytes: u64,
        addr: u64,
    ) -> Result<(), String> {
        self.map_recursive(
            spec_container,
            sel4_config,
            self.root(),
            self.get_root_level(sel4_config),
            frame_cap,
            frame_size_bytes,
            addr,
        )
    }

    fn get_leaf_level(&self, sel4_config: &Config, page_size_bytes: u64) -> usize {
        const SMALL_PAGE_BYTES: u64 = PageSize::Small as u64;
        const LARGE_PAGE_BYTES: u64 = PageSize::Large as u64;
        let levels = self.address_space_levels(sel4_config);

        match page_size_bytes {
            SMALL_PAGE_BYTES => levels - 1,
            LARGE_PAGE_BYTES => levels - 2,
            _ => unreachable!(
            "internal bug: get_pt_level_to_insert(): unknown page_size_bytes: {page_size_bytes}"
        ),
        }
    }

    fn get_level_name(&self, sel4_config: &Config, level: usize) -> String {
        match self {
            Self::VSpace { .. } => get_pt_level_name(sel4_config, level).to_string(),
            Self::IOSpace { .. } => get_iopt_level_name(level),
        }
    }

    fn get_addr_label(&self) -> &'static str {
        match self {
            Self::VSpace { .. } => "vaddr",
            Self::IOSpace { .. } => "ioaddr",
        }
    }

    fn get_leaf_bits(sel4_config: &Config) -> u64 {
        ObjectType::SmallPage.fixed_size_bits(sel4_config).unwrap()
    }

    fn level_index_bits(&self, sel4_config: &Config, level: usize) -> u64 {
        match self {
            Self::VSpace { .. } => sel4_config.vspace_level_index_bits(level),
            Self::IOSpace { .. } => {
                if level > 0 {
                    sel4_config.io_page_table_index_bits()
                } else {
                    panic!("IODevice root is not indexed by address bits");
                }
            }
        }
    }

    fn get_level_index(&self, sel4_config: &Config, level: usize, vaddr: u64) -> usize {
        if matches!(self, AddressSpace::IOSpace { .. }) && level == 0 {
            return x86_io_address_space::IOSPACE_ROOT_IOPT_SLOT;
        }
        let levels = self.address_space_levels(sel4_config);
        assert!(level < levels);

        let page_bits = Self::get_leaf_bits(sel4_config);
        let bits_from_higher_lvls: u64 = ((level + 1)..levels)
            .map(|level| self.level_index_bits(sel4_config, level))
            .sum();
        let shift = page_bits + bits_from_higher_lvls;
        let width = self.level_index_bits(sel4_config, level);
        let mask = (1u64 << width) - 1;

        ((vaddr >> shift) & mask) as usize
    }

    fn get_level_coverage(&self, sel4_config: &Config, level: usize, vaddr: u64) -> Range<u64> {
        let levels = self.address_space_levels(sel4_config);
        let page_bits = Self::get_leaf_bits(sel4_config);
        let bits_from_higher_lvls: u64 = ((level + 1)..levels)
            .map(|level| self.level_index_bits(sel4_config, level))
            .sum();

        let coverage_bits = page_bits + bits_from_higher_lvls;

        let low = (vaddr >> coverage_bits) << coverage_bits;
        let high = vaddr | ((1 << coverage_bits) - 1);

        low..high
    }

    #[allow(clippy::too_many_arguments)]
    fn map_recursive(
        &self,
        spec_container: &mut CapDLSpecContainer,
        sel4_config: &Config,
        cur_level_obj_id: ObjectId,
        cur_level: usize,
        frame_cap: Cap,
        frame_size_bytes: u64,
        addr: u64,
    ) -> Result<(), String> {
        if cur_level >= self.address_space_levels(sel4_config) {
            unreachable!("internal bug: recursed past the final address-space level");
        }

        let slot = self.get_level_index(sel4_config, cur_level, addr);
        let leaf_level = self.get_leaf_level(sel4_config, frame_size_bytes);

        if cur_level == leaf_level {
            self.insert_cap_into_level(
                spec_container,
                sel4_config,
                cur_level_obj_id,
                cur_level,
                slot,
                frame_cap,
            )
        } else {
            let next_obj_id = self.map_intermediary_level_helper(
                spec_container,
                sel4_config,
                cur_level_obj_id,
                cur_level,
                slot,
                addr,
            )?;
            self.map_recursive(
                spec_container,
                sel4_config,
                next_obj_id,
                cur_level + 1,
                frame_cap,
                frame_size_bytes,
                addr,
            )
        }
    }

    fn map_intermediary_level_helper(
        &self,
        spec_container: &mut CapDLSpecContainer,
        sel4_config: &Config,
        cur_level_obj_id: ObjectId,
        cur_level: usize,
        cur_level_slot: usize,
        addr: u64,
    ) -> Result<ObjectId, String> {
        let object = &spec_container
            .get_root_object(cur_level_obj_id)
            .unwrap()
            .object;

        self.valid_level_object(object, sel4_config, cur_level)?;
        let slots = object.slots().unwrap();

        if let Some(child_obj_id) = slots
            .iter()
            .find(|cte| usize::from(cte.slot) == cur_level_slot)
            .map(|cte| cte.cap.obj())
        {
            return Ok(child_obj_id);
        }

        let next_level = cur_level + 1;
        let next_level_coverage = self.get_level_coverage(sel4_config, next_level, addr);
        let next_level_obj = CapDLNamedObject {
            name: self
                .object_name(sel4_config, next_level, next_level_coverage.start)
                .into(),
            object: self.make_intermediate_object(next_level),
        };
        let next_obj_id = spec_container.add_root_object(next_level_obj);
        let next_cap = self.make_intermediate_cap(next_obj_id);

        self.insert_cap_into_level(
            spec_container,
            sel4_config,
            cur_level_obj_id,
            cur_level,
            cur_level_slot,
            next_cap,
        )?;

        Ok(next_obj_id)
    }

    fn insert_cap_into_level(
        &self,
        spec_container: &mut CapDLSpecContainer,
        sel4_config: &Config,
        cur_level_obj_id: ObjectId,
        cur_level: usize,
        cur_level_slot: usize,
        cap: Cap,
    ) -> Result<(), String> {
        let object = &mut spec_container
            .get_root_object_mut(cur_level_obj_id)
            .unwrap()
            .object;

        self.valid_level_object(object, sel4_config, cur_level)?;

        let slots = object.slots_mut().unwrap();

        if slots
            .iter()
            .any(|cte| usize::from(cte.slot) == cur_level_slot)
        {
            Err(format!(
                "address-space '{}': slot {} at level {} in object '{}' is already filled",
                self.name(),
                cur_level_slot,
                cur_level,
                spec_container
                    .get_root_object(cur_level_obj_id)
                    .unwrap()
                    .name
                    .as_ref()
                    .unwrap()
            ))
        } else {
            slots.push(capdl_util_make_cte(cur_level_slot as u32, cap));
            Ok(())
        }
    }

    fn make_intermediate_object(&self, level: usize) -> Object<FrameFill> {
        match self {
            &AddressSpace::VSpace { x86_ept, .. } => Object::PageTable(object::PageTable {
                x86_ept,
                is_root: false,
                level: Some(level as u8),
                slots: vec![],
            }),
            AddressSpace::IOSpace { .. } => {
                let iopt_level = level
                    .checked_sub(1)
                    .expect("Error: cannot create an intermediate IOPT for the root level.");
                Object::IOPT(object::IOPT {
                    slots: vec![],
                    level: Word(iopt_level as u64),
                })
            }
        }
    }

    fn make_intermediate_cap(&self, object: ObjectId) -> Cap {
        match self {
            AddressSpace::VSpace { .. } => Cap::PageTable(cap::PageTable { object }),
            AddressSpace::IOSpace { .. } => Cap::IOPT(cap::IOPT { object }),
        }
    }

    fn object_name(&self, sel4_config: &Config, level: usize, coverage_start: u64) -> String {
        format!(
            "{}_{}_{}_{:#x}",
            self.get_level_name(sel4_config, level),
            self.name(),
            self.get_addr_label(),
            coverage_start
        )
    }

    fn valid_level_object(
        &self,
        object: &Object<FrameFill>,
        sel4_config: &Config,
        cur_level: usize,
    ) -> Result<(), String> {
        let valid = match self {
            AddressSpace::VSpace { .. } => matches!(object, Object::PageTable(_)),
            AddressSpace::IOSpace { .. } => {
                if cur_level == 0 {
                    matches!(object, Object::IODevice(_))
                } else {
                    matches!(object, Object::IOPT(_))
                }
            }
        };
        if valid {
            Ok(())
        } else {
            Err(format!(
                "Error: found an invalid object {} at level {} in address-space {}!",
                capdl_obj_human_name(object, sel4_config),
                cur_level,
                self.name()
            ))
        }
    }

    fn address_space_levels(&self, sel4_config: &Config) -> usize {
        match self {
            AddressSpace::VSpace { .. } => sel4_config.num_page_table_levels(),
            // IOSpace level 0 is the IODevice root. Slot 0 points to the
            // normal IOPT tree root, and slots 1.. hold spare IOPTs for
            // runtime prefix levels.
            AddressSpace::IOSpace { .. } => x86_io_address_space::CAPDL_NUM_IOPT_LEVELS + 1,
        }
    }

    fn get_root_level(&self, sel4_config: &Config) -> usize {
        match self {
            AddressSpace::VSpace { .. } => sel4_config.vspace_root_level(),
            AddressSpace::IOSpace { .. } => 0,
        }
    }
}

// In future supporting SMMU can be done by matching on the IommuDeviceIdentifier or creating a new function.
pub fn create_iospace(
    spec_container: &mut CapDLSpecContainer,
    sel4_config: &Config,
    device_name: &str,
    device_identifier: IommuDeviceIdentifier,
    domain_id: Option<u64>,
) -> AddressSpace {
    let IommuDeviceIdentifier::X86Pci(pci_device) = device_identifier;

    let root = spec_container.add_root_object(CapDLNamedObject {
        name: format!("{}_{}", get_iopt_level_name(0), device_name).into(),
        object: Object::IODevice(object::IODevice {
            slots: vec![],
            domain_id: domain_id.unwrap().into(),
            pci_device: (
                Word(pci_device.bus.into()),
                Word(pci_device.device.into()),
                Word(pci_device.function.into()),
            ),
        }),
    });

    let address_space = AddressSpace::IOSpace {
        name: device_name.to_string(),
        root,
        device: device_identifier,
    };

    // The IODevice root has two roles: slot 0 is reserved for the root of the
    // normal 3-level IOPT tree, while slots 1.. hold spare IOPTs that the
    // initialiser can use when seL4 reports a wider IOVA space at runtime.
    for spare_idx in 0..x86_io_address_space::SPARE_NUM_LEVELS {
        let slot = x86_io_address_space::IOSPACE_ROOT_IOPT_SLOT + 1 + spare_idx;
        let next_obj_id = spec_container.add_root_object(CapDLNamedObject {
            name: format!(
                "{}_{}_spare_{}",
                get_iopt_level_name(1),
                address_space.name(),
                slot
            )
            .into(),
            object: address_space.make_intermediate_object(1),
        });
        let next_cap = address_space.make_intermediate_cap(next_obj_id);

        address_space
            .insert_cap_into_level(
                spec_container,
                sel4_config,
                address_space.root(),
                address_space.get_root_level(sel4_config),
                slot,
                next_cap,
            )
            .unwrap_or_else(|err| panic!("Error: create_iospace() failed allocating spare IOPT capabilities with error {err}"));
    }

    address_space
}

fn create_vspace_address_space(
    spec_container: &mut CapDLSpecContainer,
    sel4_config: &Config,
    name: &str,
    x86_ept: bool,
) -> AddressSpace {
    let root_level = sel4_config.vspace_root_level();

    let root = spec_container.add_root_object(CapDLNamedObject {
        name: format!("{}_{}", get_pt_level_name(sel4_config, root_level), name).into(),
        object: Object::PageTable(object::PageTable {
            x86_ept,
            is_root: true,
            level: Some(root_level as u8),
            slots: vec![],
        }),
    });

    AddressSpace::VSpace {
        name: name.to_string(),
        root,
        x86_ept,
    }
}

pub fn create_vspace(
    spec_container: &mut CapDLSpecContainer,
    sel4_config: &Config,
    pd_name: &str,
) -> AddressSpace {
    create_vspace_address_space(spec_container, sel4_config, pd_name, false)
}

pub fn create_vspace_ept(
    spec_container: &mut CapDLSpecContainer,
    sel4_config: &Config,
    vm_name: &str,
) -> AddressSpace {
    assert!(sel4_config.arch == Arch::X86_64);
    create_vspace_address_space(spec_container, sel4_config, vm_name, true)
}
