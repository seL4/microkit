//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//
use crate::{
    capdl::{
        spec::{capdl_object::PageTable, CapDLObject, NamedObject},
        CapDLSpec,
    },
    sel4::{Arch, Config, PageSize},
};
use sel4_capdl_initializer_types::{cap, Cap, ObjectId};
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

fn get_pt_level_index(sel4_config: &Config, level: usize, vaddr: u64) -> u64 {
    let levels = sel4_config.num_page_table_levels();

    assert!(level < levels);

    let index_bits = |level: usize| -> u64 {
        if level == top_pt_level_number(sel4_config)
            && sel4_config.arch == Arch::Aarch64
            && sel4_config.aarch64_vspace_s2_start_l1()
        {
            // Special case for first level on AArch64 platforms with hyp and 40 bits PA.
            // It have 10 bits index for VSpace.
            // match up with seL4_VSpaceBits in seL4/libsel4/sel4_arch_include/aarch64/sel4/sel4_arch/constants.h
            10
        } else {
            9
        }
    };

    let page_bits = 12;
    let bits_from_higher_lvls: u64 = ((level + 1)..levels).map(index_bits).sum();
    let shift = page_bits + bits_from_higher_lvls;
    let width = index_bits(level);
    let mask = (1u64 << width) - 1;

    (vaddr >> shift) & mask
}

fn get_pt_level_coverage(sel4_config: &Config, level: usize, vaddr: u64) -> Range<u64> {
    let levels = sel4_config.num_page_table_levels() as u64;
    let page_bits = 12;
    let bits_from_higher_lvls: u64 = (levels - (level as u64)) * 9;

    let coverage_bits = page_bits + bits_from_higher_lvls;

    let low = (vaddr >> coverage_bits) << coverage_bits;
    let high = vaddr | ((1 << coverage_bits) - 1);

    low..high
}

fn get_pt_level_to_insert(sel4_config: &Config, page_size_bytes: u64) -> usize {
    const SMALL_PAGE_BYTES: u64 = PageSize::Small as u64;
    const LARGE_PAGE_BYTES: u64 = PageSize::Large as u64;
    match page_size_bytes {
        SMALL_PAGE_BYTES => sel4_config.num_page_table_levels() - 1,
        LARGE_PAGE_BYTES => sel4_config.num_page_table_levels() - 2,
        _ => unreachable!(
            "internal bug: get_pt_level_to_insert(): unknown page_size_bytes: {page_size_bytes}"
        ),
    }
}

fn top_pt_level_number(sel4_config: &Config) -> usize {
    if sel4_config.arch == Arch::Aarch64 && sel4_config.aarch64_vspace_s2_start_l1() {
        1
    } else {
        0
    }
}

fn insert_cap_into_page_table_level(
    spec: &mut CapDLSpec,
    cur_level_obj_id: ObjectId,
    cur_level: usize,
    cur_level_slot: u64,
    cap: Cap,
) -> Result<(), String> {
    let page_table_level_obj_wrapper = spec.get_root_object_mut(cur_level_obj_id).unwrap();
    if let CapDLObject::PageTable(page_table_object) = &mut page_table_level_obj_wrapper.object {
        // Sanity check that this slot is free
        match page_table_object
            .slots
            .iter()
            .find(|cte| cte.0 == cur_level_slot as usize)
        {
            Some(_) => Err(format!(
                "insert_cap_into_page_table_level(): internal bug: slot {} at PT level {} with name '{}' already filled",
                cur_level_slot, cur_level, spec.get_root_object(cur_level_obj_id).unwrap().name
            )),
            None => {
                page_table_object.slots.push((cur_level_slot as usize, cap));
                Ok(())
            }
        }
    } else {
        Err(format!(
            "insert_cap_into_page_table_level(): internal bug: received a non-Page Table object id {} with name '{}'",
            cur_level_obj_id, spec.get_root_object(cur_level_obj_id).unwrap().name
        ))
    }
}

// Just this one time pinky promise
#[allow(clippy::too_many_arguments)]
fn map_intermediary_level_helper(
    spec: &mut CapDLSpec,
    sel4_config: &Config,
    pd_name: &str,
    next_level_name_prefix: &str,
    vspace_obj_id: ObjectId,
    cur_level_obj_id: ObjectId,
    cur_level: usize,
    cur_level_slot: u64,
    vaddr: u64,
) -> Result<ObjectId, String> {
    let page_table_level_obj_wrapper = spec.get_root_object(cur_level_obj_id).unwrap();
    if let CapDLObject::PageTable(page_table_object) = &page_table_level_obj_wrapper.object {
        match page_table_object
            .slots
            .iter()
            .find(|cte| cte.0 == cur_level_slot as usize)
        {
            Some(cte_unwrapped) => {
                // Next level object already created, nothing to do here
                return Ok(cte_unwrapped.1.obj());
            }
            None => {
                // We need to create the next level paging structure, get out of this scope for now
                // so we don't get a double mutable borrow of spec when we need to insert the next level object
            }
        }
    } else {
        return Err(format!("map_intermediary_level_helper(): internal bug: received a non-Page Table object id {} with name '{}', for mapping at level {}, to pd {}.",
            cur_level_obj_id, spec.get_root_object(cur_level_obj_id).unwrap().name, cur_level, pd_name));
    }

    // Next level object not already created, create it.
    let vspace_obj = match &spec.get_root_object(vspace_obj_id).unwrap().object {
        CapDLObject::PageTable(o) => o,
        _ => unreachable!(
            "map_intermediary_level_helper(): internal bug: received a non VSpace object id {} with name '{}'",
            vspace_obj_id, spec.get_root_object(vspace_obj_id).unwrap().name
        ),
    };
    let next_level_coverage = get_pt_level_coverage(sel4_config, cur_level + 1, vaddr);
    let next_level_inner_obj = PageTable {
        x86_ept: vspace_obj.x86_ept,
        is_root: false, // because the VSpace has already been created separately
        level: Some(cur_level as u8 + 1),
        slots: [].to_vec(),
    };
    // We create name with this PT level coverage so that every object names are unique
    let next_level_object = NamedObject {
        name: format!(
            "{}_{}_vaddr_0x{:x}",
            next_level_name_prefix, pd_name, next_level_coverage.start
        ),
        object: CapDLObject::PageTable(next_level_inner_obj),
        expected_alloc: None,
    };
    let next_level_obj_id = spec.add_root_object(next_level_object);
    let next_level_cap = Cap::PageTable(cap::PageTable {
        object: next_level_obj_id,
    });

    // Then insert into the correct slot at the current level, return and continue mapping
    match insert_cap_into_page_table_level(
        spec,
        cur_level_obj_id,
        cur_level,
        cur_level_slot,
        next_level_cap,
    ) {
        Ok(_) => Ok(next_level_obj_id),
        Err(err_reason) => Err(err_reason),
    }
}

pub fn create_vspace(spec: &mut CapDLSpec, sel4_config: &Config, pd_name: &str) -> ObjectId {
    spec.add_root_object(NamedObject {
        name: format!(
            "{}_{}",
            get_pt_level_name(sel4_config, top_pt_level_number(sel4_config)),
            pd_name
        ),
        object: CapDLObject::PageTable(PageTable {
            x86_ept: false,
            is_root: true,
            level: Some(top_pt_level_number(sel4_config) as u8),
            slots: [].to_vec(),
        }),
        expected_alloc: None,
    })
}

pub fn create_vspace_ept(spec: &mut CapDLSpec, sel4_config: &Config, vm_name: &str) -> ObjectId {
    assert!(sel4_config.arch == Arch::X86_64);

    spec.add_root_object(NamedObject {
        name: format!("{}_{}", get_pt_level_name(sel4_config, 0), vm_name),
        object: CapDLObject::PageTable(PageTable {
            x86_ept: true,
            is_root: true,
            level: Some(top_pt_level_number(sel4_config) as u8),
            slots: [].to_vec(),
        }),
        expected_alloc: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn map_recursive(
    spec: &mut CapDLSpec,
    sel4_config: &Config,
    pd_name: &str,
    vspace_obj_id: ObjectId,
    pt_obj_id: ObjectId,
    cur_level: usize,
    frame_cap: Cap,
    frame_size_bytes: u64,
    vaddr: u64,
) -> Result<(), String> {
    if cur_level >= sel4_config.num_page_table_levels() {
        unreachable!("internal bug: we should have never recursed further!");
    }

    let this_level_index = get_pt_level_index(sel4_config, cur_level, vaddr);

    if cur_level == get_pt_level_to_insert(sel4_config, frame_size_bytes) {
        // Base case: we got to the target level to insert the frame cap.
        insert_cap_into_page_table_level(spec, pt_obj_id, cur_level, this_level_index, frame_cap)
    } else {
        // Recursive case: we have not gotten to the correct level, create the next level and recurse down.
        let next_level_name_prefix = get_pt_level_name(sel4_config, cur_level + 1);
        match map_intermediary_level_helper(
            spec,
            sel4_config,
            pd_name,
            next_level_name_prefix,
            vspace_obj_id,
            pt_obj_id,
            cur_level,
            this_level_index,
            vaddr,
        ) {
            Ok(next_level_pt_obj_id) => map_recursive(
                spec,
                sel4_config,
                pd_name,
                vspace_obj_id,
                next_level_pt_obj_id,
                cur_level + 1,
                frame_cap,
                frame_size_bytes,
                vaddr,
            ),
            Err(err_reason) => Err(err_reason),
        }
    }
}

pub fn map_page(
    spec: &mut CapDLSpec,
    sel4_config: &Config,
    pd_name: &str,
    vspace_obj_id: ObjectId,
    frame_cap: Cap,
    frame_size_bytes: u64,
    vaddr: u64,
) -> Result<(), String> {
    map_recursive(
        spec,
        sel4_config,
        pd_name,
        vspace_obj_id,
        vspace_obj_id,
        top_pt_level_number(sel4_config),
        frame_cap,
        frame_size_bytes,
        vaddr,
    )
}
