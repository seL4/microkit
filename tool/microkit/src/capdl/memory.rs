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
    sel4::{Arch, Config, ObjectType, PageSize},
};
use sel4_capdl_initializer_types::{cap, object, Cap, Object, ObjectId};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressSpace {
    VSpace {
        name: String,
        root: ObjectId,
        x86_ept: bool,
    },
}

impl AddressSpace {
    pub fn root(&self) -> ObjectId {
        match self {
            &Self::VSpace { root, .. } => root,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::VSpace { name, .. } => name,
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
        }
    }

    fn get_addr_label(&self) -> &'static str {
        match self {
            Self::VSpace { .. } => "vaddr",
        }
    }

    fn get_leaf_bits(&self, sel4_config: &Config) -> u64 {
        match self {
            Self::VSpace { .. } => ObjectType::SmallPage.fixed_size_bits(sel4_config).unwrap(),
        }
    }

    fn level_index_bits(&self, sel4_config: &Config, level: usize) -> u64 {
        match self {
            Self::VSpace { .. } => sel4_config.vspace_level_index_bits(level),
        }
    }

    fn get_level_index(&self, sel4_config: &Config, level: usize, vaddr: u64) -> usize {
        let levels = self.address_space_levels(sel4_config);
        assert!(level < levels);

        let page_bits = self.get_leaf_bits(sel4_config);
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
        let page_bits = self.get_leaf_bits(sel4_config);
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
        }
    }

    fn make_intermediate_cap(&self, object: ObjectId) -> Cap {
        match self {
            AddressSpace::VSpace { .. } => Cap::PageTable(cap::PageTable { object }),
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
        }
    }

    fn get_root_level(&self, sel4_config: &Config) -> usize {
        match self {
            AddressSpace::VSpace { .. } => sel4_config.vspace_root_level(),
        }
    }
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
