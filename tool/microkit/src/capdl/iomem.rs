use std::ops::Range;

use sel4_capdl_initializer_types::{cap, object, Cap, Object, ObjectId};

use crate::{
    capdl::{util::capdl_util_make_cte, CapDLNamedObject, CapDLSpecContainer},
    sel4::Config,
};

// VTD Page table level is defaulted to the three-level structure even if the hardware supports four level.
// Sel4 will only attempt to use the four-level structure if the hardware does not supports three level.
// https://github.com/seL4/seL4/blob/c406015c389decc4559fd44cb69604ddd24a0ddb/src/plat/pc99/machine/intel-vtd.c#L498
const VTD_PML4_LEVEL: u8 = 0;
const VTD_PAGE_TABLE_LEVEL: u8 = 4;
const VTD_SEL4_DEFAULT_PT_LEVEL: u8 = 3;
const VTD_BITS_PER_LEVEL: u8 = 9;
const VTD_ENTRY_BITS: u8 = 12;
pub(crate) const VTD_MAX_ADDR: u64 =
    (1 << (VTD_BITS_PER_LEVEL * VTD_SEL4_DEFAULT_PT_LEVEL + VTD_ENTRY_BITS)) - 1;

pub fn create_iospace(
    spec_container: &mut CapDLSpecContainer,
    sel4_config: &Config,
    iospace_name: &str,
    pci_bus: u8,
    pci_device: u8,
    dev_func: u8,
    pd_id: usize,
) -> ObjectId {
    let id = spec_container.add_root_object(CapDLNamedObject {
        name: format!(
            "{}_{}",
            get_iopt_level_name(sel4_config, VTD_PML4_LEVEL),
            iospace_name,
        )
        .into(),
        object: Object::IOPageTable(object::IOPageTable {
            is_root: true,
            level: Some(VTD_PML4_LEVEL),
            slots: [].to_vec(),
        }),
    });

    const PD_TO_DOMAIN_ID_OFFSET: u16 = 1;

    spec_container.add_root_object(CapDLNamedObject {
        name: format!("IOSpace_{}", iospace_name,).into(),
        object: Object::IOSpace(object::IOSpace {
            pci_bus,
            pci_device,
            dev_func,
            domain_id: u16::try_from(pd_id)
                .unwrap_or_else(|_| panic!("The pd id {} is too large!", pd_id))
                + PD_TO_DOMAIN_ID_OFFSET,
            slots: vec![capdl_util_make_cte(
                0,
                Cap::IOPageTable(cap::IOPageTable { object: id }),
            )],
        }),
    });

    id
}

pub fn map_io_page(
    spec_container: &mut CapDLSpecContainer,
    sel4_config: &Config,
    iospace_name: &str,
    iospace_obj_id: ObjectId,
    frame_cap: Cap,
    ioaddr: u64,
) -> Result<(), String> {
    map_recursive(
        spec_container,
        sel4_config,
        iospace_name,
        iospace_obj_id,
        VTD_PML4_LEVEL,
        frame_cap,
        ioaddr,
    )
}

fn get_iopt_level_name(sel4_config: &Config, level: u8) -> &str {
    match sel4_config.arch {
        crate::sel4::Arch::X86_64 => match level {
            0 => "VTD_pml4",
            1 => "VTD_pdpt",
            2 => "VTD_pd",
            3 => "VTD_pt",
            _ => unreachable!(),
        },
        _ => unreachable!("get_iopt_level_name(): Internal bug: Only x86 support iommu!"),
    }
}

#[allow(clippy::too_many_arguments)]
fn map_intermediary_level_helper(
    spec_container: &mut CapDLSpecContainer,
    sel4_config: &Config,
    iospace_name: &str,
    next_level_name_prefix: &str,
    cur_level_obj_id: ObjectId,
    cur_level: u8,
    cur_level_slot: usize,
    ioaddr: u64,
) -> Result<ObjectId, String> {
    let page_table_level_obj_wrapper = spec_container.get_root_object(cur_level_obj_id).unwrap();
    if let Object::IOPageTable(page_table_object) = &page_table_level_obj_wrapper.object {
        match page_table_object
            .slots
            .iter()
            .find(|cte| usize::from(cte.slot) == cur_level_slot)
        {
            Some(cte_unwrapped) => {
                // Next level object already created, nothing to do here
                return Ok(cte_unwrapped.cap.obj());
            }
            None => {
                // We need to create the next level paging structure, get out of this scope for now
                // so we don't get a double mutable borrow of spec when we need to insert the next level object
            }
        }
    } else {
        return Err(format!("map_intermediary_level_helper(): internal bug: received a non-Page Table object id {} with name '{}', for mapping at level {}, to pd {}.",
            usize::from(cur_level_obj_id), spec_container.get_root_object(cur_level_obj_id).unwrap().name.as_ref().unwrap(), cur_level, iospace_name));
    }

    // get_pt_level_coverage works the same for io memory as well
    let next_level_coverage = get_io_pt_level_coverage(sel4_config, cur_level + 1, ioaddr);
    let next_level_inner_obj = object::IOPageTable {
        is_root: false, // IOSpace is always created seperately
        level: Some(cur_level + 1),
        slots: [].to_vec(),
    };
    // We create name with this PT level coverage so that every object names are unique
    let next_level_object = CapDLNamedObject {
        name: format!(
            "{}_{}_ioaddr_0x{:x}",
            next_level_name_prefix, iospace_name, next_level_coverage.start
        )
        .into(),
        object: Object::IOPageTable(next_level_inner_obj),
    };
    let next_level_obj_id = spec_container.add_root_object(next_level_object);
    let next_level_cap = Cap::IOPageTable(cap::IOPageTable {
        object: next_level_obj_id,
    });

    // Then insert into the correct slot at the current level, return and continue mapping
    match insert_cap_into_io_page_table_level(
        spec_container,
        cur_level_obj_id,
        cur_level,
        cur_level_slot,
        next_level_cap,
    ) {
        Ok(_) => Ok(next_level_obj_id),
        Err(err_reason) => Err(err_reason),
    }
}

fn insert_cap_into_io_page_table_level(
    spec_container: &mut CapDLSpecContainer,
    cur_level_obj_id: ObjectId,
    cur_level: u8,
    cur_level_slot: usize,
    cap: Cap,
) -> Result<(), String> {
    let page_table_level_obj_wrapper = spec_container
        .get_root_object_mut(cur_level_obj_id)
        .unwrap();
    if let Object::IOPageTable(page_table_object) = &mut page_table_level_obj_wrapper.object {
        // Sanity check that this slot is free
        match page_table_object
            .slots
            .iter()
            .find(|cte| usize::from(cte.slot) == cur_level_slot)
        {
            Some(_) => Err(format!(
                "insert_cap_into_io_page_table_level(): internal bug: slot {} at PT level {} with name '{}' already filled",
                cur_level_slot, cur_level, spec_container.get_root_object(cur_level_obj_id).unwrap().name.as_ref().unwrap()
            )),
            None => {
                page_table_object.slots.push(capdl_util_make_cte(cur_level_slot as u32, cap));
                Ok(())
            }
        }
    } else {
        Err(format!(
            "insert_cap_into_io_page_table_level(): internal bug: received a non-Page Table object id {} with name '{}'",
            usize::from(cur_level_obj_id), spec_container.get_root_object(cur_level_obj_id).unwrap().name.as_ref().unwrap()
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn map_recursive(
    spec_container: &mut CapDLSpecContainer,
    sel4_config: &Config,
    pd_name: &str,
    pt_obj_id: ObjectId,
    cur_level: u8,
    frame_cap: Cap,
    ioaddr: u64,
) -> Result<(), String> {
    if cur_level >= VTD_PAGE_TABLE_LEVEL {
        unreachable!("internal bug: we should have never recursed further!");
    }

    let this_level_index = get_io_pt_level_index(sel4_config, cur_level, ioaddr);

    if cur_level == VTD_PAGE_TABLE_LEVEL - 1 {
        // Base case: we got to the target level to insert the frame cap.
        insert_cap_into_io_page_table_level(
            spec_container,
            pt_obj_id,
            cur_level,
            this_level_index,
            frame_cap,
        )
    } else {
        // Recursive case: we have not gotten to the correct level, create the next level and recurse down.
        let next_level_name_prefix = get_iopt_level_name(sel4_config, cur_level + 1);
        match map_intermediary_level_helper(
            spec_container,
            sel4_config,
            pd_name,
            next_level_name_prefix,
            pt_obj_id,
            cur_level,
            this_level_index,
            ioaddr,
        ) {
            Ok(next_level_pt_obj_id) => map_recursive(
                spec_container,
                sel4_config,
                pd_name,
                next_level_pt_obj_id,
                cur_level + 1,
                frame_cap,
                ioaddr,
            ),
            Err(err_reason) => Err(err_reason),
        }
    }
}

fn get_io_pt_level_index(sel4_config: &Config, level: u8, ioaddr: u64) -> usize {
    match sel4_config.arch {
        crate::sel4::Arch::X86_64 => {
            assert!(level < VTD_PAGE_TABLE_LEVEL);

            let shift = VTD_BITS_PER_LEVEL * (VTD_PAGE_TABLE_LEVEL - level) - VTD_BITS_PER_LEVEL
                + VTD_ENTRY_BITS;

            ((ioaddr >> shift) & ((1u64 << VTD_BITS_PER_LEVEL) - 1)) as usize
        }
        crate::sel4::Arch::Aarch64 => {
            unreachable!("Internal bug: Aarch64 is not supported for IOMMU")
        }
        crate::sel4::Arch::Riscv64 => {
            unreachable!("Internal bug: Riscv64 is not supported for IOMMU")
        }
    }
}

fn get_io_pt_level_coverage(sel4_config: &Config, level: u8, ioaddr: u64) -> Range<u64> {
    match sel4_config.arch {
        crate::sel4::Arch::X86_64 => {
            let bits_from_higher_lvls: u64 =
                (VTD_PAGE_TABLE_LEVEL as u64 - (level as u64)) * VTD_BITS_PER_LEVEL as u64;
            let coverage_bits = VTD_BITS_PER_LEVEL as u64 + bits_from_higher_lvls;
            let low = (ioaddr >> coverage_bits) << coverage_bits;
            let high = ioaddr | ((1 << coverage_bits) - 1);
            low..high
        }
        _ => unreachable!(
            "get_io_pt_level_coverage(): Internal bug: IOMMU is only supported for x86!"
        ),
    }
}
