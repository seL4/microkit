//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::ops::Range;

use crate::elf::ElfSegmentData;
use crate::util::round_up;
use crate::{elf::ElfFile, sel4::PageSize};
use crate::{serialise_ut, UntypedObject};

// The capDL initialiser heap size is calculated by:
// (spec size * multiplier) + INITIALISER_HEAP_ADD_ON_CONSTANT
pub const DEFAULT_INITIALISER_HEAP_MULTIPLIER: f64 = 2.0;
const INITIALISER_HEAP_ADD_ON_CONSTANT: u64 = 16 * 4096;
// Page size used for allocating the spec and heap segments.
pub const INITIALISER_GRANULE_SIZE: PageSize = PageSize::Small;

pub struct CapDLInitialiserSpecMetadata {
    pub spec_size: u64,
    pub heap_size: u64,
}

pub struct CapDLInitialiser {
    pub elf: ElfFile,
    pub heap_multiplier: f64,
    pub spec_metadata: Option<CapDLInitialiserSpecMetadata>,
}

impl CapDLInitialiser {
    pub fn new(elf: ElfFile, heap_multiplier: f64) -> CapDLInitialiser {
        CapDLInitialiser {
            elf,
            heap_multiplier,
            spec_metadata: None,
        }
    }

    pub fn image_bound(&self) -> Range<u64> {
        self.elf.lowest_vaddr()..round_up(self.elf.highest_vaddr(), INITIALISER_GRANULE_SIZE as u64)
    }

    pub fn add_spec(&mut self, payload: Vec<u8>) {
        if self.spec_metadata.is_some() {
            unreachable!("internal bug: CapDLInitialiser::add_spec() called more than once");
        }

        let spec_vaddr = self.elf.next_vaddr(INITIALISER_GRANULE_SIZE);
        let spec_size = payload.len() as u64;
        self.elf.add_segment(
            true,
            false,
            false,
            spec_vaddr,
            ElfSegmentData::RealData(payload),
        );

        // These symbol names must match rust-sel4/crates/sel4-capdl-initializer/src/main.rs
        self.elf
            .write_symbol(
                "sel4_capdl_initializer_serialized_spec_start",
                &spec_vaddr.to_le_bytes(),
            )
            .unwrap();
        self.elf
            .write_symbol(
                "sel4_capdl_initializer_serialized_spec_size",
                &spec_size.to_le_bytes(),
            )
            .unwrap();

        // Very important to make the heap the last region in memory so we can optimise the bootable image size later.
        let heap_vaddr = self.elf.next_vaddr(INITIALISER_GRANULE_SIZE);
        let heap_size = round_up(
            (spec_size as f64 * self.heap_multiplier) as u64 + INITIALISER_HEAP_ADD_ON_CONSTANT,
            INITIALISER_GRANULE_SIZE as u64,
        );
        self.elf.add_segment(
            true,
            true,
            false,
            heap_vaddr,
            ElfSegmentData::UninitialisedData(heap_size),
        );
        self.elf
            .write_symbol(
                "sel4_capdl_initializer_heap_start",
                &heap_vaddr.to_le_bytes(),
            )
            .unwrap();
        self.elf
            .write_symbol("sel4_capdl_initializer_heap_size", &heap_size.to_le_bytes())
            .unwrap();

        self.elf
            .write_symbol(
                "sel4_capdl_initializer_image_start",
                &self.elf.lowest_vaddr().to_le_bytes(),
            )
            .unwrap();
        self.elf
            .write_symbol(
                "sel4_capdl_initializer_image_end",
                &self.elf.highest_vaddr().to_le_bytes(),
            )
            .unwrap();

        self.spec_metadata = Some(CapDLInitialiserSpecMetadata {
            spec_size,
            heap_size,
        });
    }

    pub fn spec_metadata(&self) -> &Option<CapDLInitialiserSpecMetadata> {
        &self.spec_metadata
    }

    pub fn have_spec(&self) -> bool {
        self.spec_metadata.is_some()
    }

    pub fn replace_spec(&mut self, new_payload: Vec<u8>) {
        if self.spec_metadata.is_none() {
            unreachable!("internal bug: CapDLInitialiser::replace_spec() called when no spec have been added before");
        }

        self.elf.segments.pop();
        self.elf.segments.pop();
        self.add_spec(new_payload);
    }

    pub fn add_expected_untypeds(&mut self, untypeds: &[UntypedObject]) {
        let mut uts_desc: Vec<u8> = Vec::new();
        for ut in untypeds.iter() {
            uts_desc.extend(serialise_ut(ut));
        }

        self.elf
            .write_symbol(
                "sel4_capdl_initializer_expected_untypeds_list_num_entries",
                &(untypeds.len() as u64).to_le_bytes(),
            )
            .unwrap();
        self.elf
            .write_symbol("sel4_capdl_initializer_expected_untypeds_list", &uts_desc)
            .unwrap();
    }
}
