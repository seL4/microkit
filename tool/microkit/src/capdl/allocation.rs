//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::ops::Range;

use sel4_capdl_initializer_types::ObjectId;

use crate::{
    capdl::{spec::capdl_obj_physical_size_bits, CapDLSpecContainer, ExpectedAllocation},
    sel4::{BootInfo, Config},
    util::human_size_strict,
    UntypedObject,
};

pub enum CapDLAllocEmulationErrorLevel {
    Suppressed,
    PrintStderr,
}

/// For the given spec and list of untypeds, simulate the CapDL initialiser's
/// object allocation algorithm. Record each object's paddr and UT's index in
/// its `expected_alloc` struct field. Assumes that the spec objects are sorted
/// by paddr, then by size
///
/// Returns `true` if all objects can be allocated, `false` otherwise.
pub fn simulate_capdl_object_alloc_algorithm(
    spec_container: &mut CapDLSpecContainer,
    kernel_boot_info: &BootInfo,
    kernel_config: &Config,
    error_reporting_level: CapDLAllocEmulationErrorLevel,
) -> bool {
    // Step 1: sort untypeds by paddr.
    // We don't want to mess with the original order in `kernel_boot_info` as we will patch
    // them out to the initialiser later.
    let mut untypeds_by_paddr: Vec<(usize, &UntypedObject)> = kernel_boot_info
        .untyped_objects
        .iter()
        .enumerate()
        .collect();
    untypeds_by_paddr.sort_by_key(|(_, ut)| ut.base());

    // Step 2: create object "windows" for objects that doesn't specify paddr,
    // where each window contains all objects of the array index size bits.
    let mut object_windows_by_size: Vec<Option<Range<usize>>> =
        vec![None; kernel_config.word_size as usize];
    let first_obj_id_without_paddr = spec_container
        .spec
        .objects
        .partition_point(|named_obj| named_obj.object.paddr().is_some());
    for (id, named_object) in spec_container.spec.objects[first_obj_id_without_paddr..]
        .iter()
        .enumerate()
    {
        let phys_size_bit =
            capdl_obj_physical_size_bits(&named_object.object, kernel_config) as usize;
        if phys_size_bit > 0 {
            let window_maybe = object_windows_by_size.get_mut(phys_size_bit).unwrap();
            match window_maybe {
                Some(window) => window.end += 1,
                None => {
                    let _ = window_maybe.insert(
                        first_obj_id_without_paddr + id..first_obj_id_without_paddr + id + 1,
                    );
                }
            }
        }
    }

    // Step 3: Sanity check that all objects with a paddr attached can be allocated.
    let mut phys_addrs_ok = true;
    for obj_with_paddr_id in 0..first_obj_id_without_paddr {
        let named_obj = &spec_container.spec.objects[obj_with_paddr_id];
        let paddr_base = u64::from(named_obj.object.paddr().unwrap());

        let obj_size_bytes =
            1 << capdl_obj_physical_size_bits(&named_obj.object, kernel_config) as usize;
        let paddr_range = paddr_base..paddr_base + obj_size_bytes;

        // Binary search for the UT that is next to the UT that might fit.
        // i.e. we are looking for the first UT that is uts[i_ut].paddr() > paddr_range.start
        let ut_after_candidate_idx =
            untypeds_by_paddr.partition_point(|(_, ut)| ut.base() <= paddr_range.start);

        if ut_after_candidate_idx == 0 {
            // Predicate returned false for the first UT, cannot allocate this object as all UTs are
            // after the object.
            phys_addrs_ok = false;
        } else {
            let candidate_ut = &untypeds_by_paddr[ut_after_candidate_idx - 1].1;
            let candidate_ut_range =
                candidate_ut.base()..candidate_ut.base() + (1 << candidate_ut.size_bits());
            if !(candidate_ut_range.start <= paddr_range.start
                && candidate_ut_range.end >= paddr_range.end)
            {
                if matches!(
                    error_reporting_level,
                    CapDLAllocEmulationErrorLevel::PrintStderr
                ) {
                    eprintln!("ERROR: object '{}', with paddr 0x{:0>12x}..0x{:0>12x} is not in any valid memory region.", named_obj.name.as_ref().unwrap(), paddr_range.start, paddr_range.end);
                }
                phys_addrs_ok = false;
            }
        }
    }

    if !phys_addrs_ok
        && matches!(
            error_reporting_level,
            CapDLAllocEmulationErrorLevel::PrintStderr
        )
    {
        eprintln!("Below are the valid ranges of memory to be allocated from:");
        eprintln!("Valid ranges outside of main memory:");
        for (_i, ut) in untypeds_by_paddr.iter().filter(|(_i, ut)| ut.is_device) {
            eprintln!("     [0x{:0>12x}..0x{:0>12x})", ut.base(), ut.end());
        }
        eprintln!("Valid ranges within main memory:");
        for (_i, ut) in untypeds_by_paddr.iter().filter(|(_i, ut)| !ut.is_device) {
            eprintln!("     [0x{:0>12x}..0x{:0>12x})", ut.base(), ut.end());
        }
        return false;
    }

    let num_objs_with_paddr = first_obj_id_without_paddr;
    let mut next_obj_id_with_paddr = 0;
    for (ut_orig_idx, ut) in untypeds_by_paddr.iter() {
        let mut cur_paddr = ut.base();

        loop {
            // If this untyped covers frames that specify a paddr, don't allocate ordinary objects
            // past the lowest frame's paddr.
            let target = if next_obj_id_with_paddr < num_objs_with_paddr {
                ut.end().min(u64::from(
                    spec_container
                        .spec
                        .objects
                        .get(next_obj_id_with_paddr)
                        .unwrap()
                        .object
                        .paddr()
                        .unwrap(),
                ))
            } else {
                ut.end()
            };
            let target_is_obj_with_paddr = target < ut.end();

            while cur_paddr < target {
                let max_size_bits = usize::try_from(cur_paddr.trailing_zeros())
                    .unwrap()
                    .min((target - cur_paddr).trailing_zeros().try_into().unwrap());
                let mut created = false;

                // If this UT is in main memory, allocate all the objects that does not specify a paddr first.
                if !ut.is_device {
                    // Greedily create a largest possible objects that would fit in this untyped.
                    // If at the current size we cannot allocate any more object, drop to objects of smaller
                    // size that still need to be allocated.
                    for size_bits in (0..=max_size_bits).rev() {
                        let obj_id_range_maybe = object_windows_by_size.get_mut(size_bits).unwrap();
                        if obj_id_range_maybe.is_some() {
                            // Got objects at this size bits, check if we still have any to allocate
                            if obj_id_range_maybe.as_ref().unwrap().start
                                < obj_id_range_maybe.as_ref().unwrap().end
                            {
                                let obj_id: ObjectId =
                                    obj_id_range_maybe.as_ref().unwrap().start.into();

                                {
                                    // Should not have touched this object before
                                    assert!(!spec_container
                                        .expected_allocations
                                        .contains_key(&obj_id));
                                    // Book-keep where this object will be allocated so we can write the details out to the report later.
                                    spec_container.expected_allocations.insert(
                                        obj_id,
                                        ExpectedAllocation {
                                            ut_idx: *ut_orig_idx,
                                            paddr: cur_paddr,
                                        },
                                    );
                                }

                                let named_obj = spec_container.get_root_object(obj_id).unwrap();

                                cur_paddr += 1
                                    << capdl_obj_physical_size_bits(
                                        &named_obj.object,
                                        kernel_config,
                                    ) as usize;
                                obj_id_range_maybe.as_mut().unwrap().start += 1;
                                created = true;
                                break;
                            }
                        }
                    }
                }
                if !created {
                    if target_is_obj_with_paddr {
                        // Manipulate the untyped's watermark to allocate at the correct paddr.
                        cur_paddr += 1 << max_size_bits;
                    } else {
                        cur_paddr = target;
                    }
                }
            }
            if target_is_obj_with_paddr {
                {
                    // Should not have touched this object before
                    assert!(!spec_container
                        .expected_allocations
                        .contains_key(&next_obj_id_with_paddr.into()));
                    // Book-keep where this object will be allocated so we can write the details out to the report later.
                    spec_container.expected_allocations.insert(
                        next_obj_id_with_paddr.into(),
                        ExpectedAllocation {
                            ut_idx: *ut_orig_idx,
                            paddr: cur_paddr,
                        },
                    );
                }

                // Watermark now at the correct level, make the actual object
                let named_obj = spec_container
                    .get_root_object(next_obj_id_with_paddr.into())
                    .unwrap();

                assert_eq!(u64::from(named_obj.object.paddr().unwrap()), cur_paddr);

                cur_paddr +=
                    1 << capdl_obj_physical_size_bits(&named_obj.object, kernel_config) as usize;
                next_obj_id_with_paddr += 1;
            } else {
                break;
            }
        }
    }

    // Ensure that we've created every objects
    let mut oom = false;
    for size_bit in 0..kernel_config.word_size {
        let obj_id_range_maybe = object_windows_by_size.get(size_bit as usize).unwrap();
        if obj_id_range_maybe.is_some() {
            let obj_id_range = obj_id_range_maybe.as_ref().unwrap();
            if obj_id_range.start != obj_id_range.end {
                oom = true;
                let shortfall = (obj_id_range.end - obj_id_range.start) as u64;
                let individual_sz = (1 << size_bit) as u64;
                if matches!(
                    error_reporting_level,
                    CapDLAllocEmulationErrorLevel::PrintStderr
                ) {
                    eprintln!(
                        "ERROR: ran out of untypeds for allocating objects of size {}, still need to create {} objects which requires {} of additional memory.",
                        human_size_strict(individual_sz), shortfall, human_size_strict(individual_sz * shortfall)
                    );
                }
            }
        }
    }

    !oom
}
