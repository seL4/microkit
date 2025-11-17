//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::ops::Range;

use rkyv::util::AlignedVec;

use crate::elf::ElfSegmentData;
use crate::util::round_up;
use crate::{elf::ElfFile, sel4::PageSize};
use crate::{serialise_ut, UntypedObject};

// Page size used for allocating the spec and heap segments.
pub const INITIALISER_GRANULE_SIZE: PageSize = PageSize::Small;

pub struct CapDLInitialiserSpecMetadata {
    pub spec_size: u64,
}

pub struct CapDLInitialiser {
    pub elf: ElfFile,
    pub phys_base: Option<u64>,
    pub spec_metadata: Option<CapDLInitialiserSpecMetadata>,
}

impl CapDLInitialiser {
    pub fn new(elf: ElfFile) -> CapDLInitialiser {
        CapDLInitialiser {
            elf,
            phys_base: None,
            spec_metadata: None,
        }
    }

    pub fn image_bound(&self) -> Range<u64> {
        self.elf.lowest_vaddr()..round_up(self.elf.highest_vaddr(), INITIALISER_GRANULE_SIZE as u64)
    }

    pub fn add_spec(&mut self, spec_payload: AlignedVec, embedded_frame_data: Vec<u8>) {
        if self.spec_metadata.is_some() {
            self.elf.segments.pop();
            self.elf.segments.pop();
            self.spec_metadata = None;
        }

        let spec_vaddr = self.elf.next_vaddr(INITIALISER_GRANULE_SIZE);
        let spec_size = spec_payload.len() as u64;
        self.elf.add_segment(
            true,
            false,
            false,
            spec_vaddr,
            ElfSegmentData::RealData(spec_payload.into()),
        );

        let embedded_frame_data_vaddr = self.elf.next_vaddr(INITIALISER_GRANULE_SIZE);
        self.elf.add_segment(
            true,
            false,
            false,
            embedded_frame_data_vaddr,
            ElfSegmentData::RealData(embedded_frame_data),
        );

        // These symbol names must match rust-sel4/crates/sel4-capdl-initializer/src/main.rs
        self.elf
            .write_symbol(
                "sel4_capdl_initializer_embedded_frames_data_start",
                &embedded_frame_data_vaddr.to_le_bytes(),
            )
            .unwrap();

        self.elf
            .write_symbol(
                "sel4_capdl_initializer_serialized_spec_data_start",
                &spec_vaddr.to_le_bytes(),
            )
            .unwrap();
        self.elf
            .write_symbol(
                "sel4_capdl_initializer_serialized_spec_data_size",
                &spec_size.to_le_bytes(),
            )
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

        self.spec_metadata = Some(CapDLInitialiserSpecMetadata { spec_size });
    }

    pub fn spec_metadata(&self) -> &Option<CapDLInitialiserSpecMetadata> {
        &self.spec_metadata
    }

    pub fn add_expected_untypeds(&mut self, untypeds: &[UntypedObject]) {
        let mut uts_desc: Vec<u8> = Vec::new();
        for ut in untypeds.iter() {
            uts_desc.extend(serialise_ut(ut));
        }

        // This feature is currently not in mainline rust-seL4.
        if self
            .elf
            .find_symbol("sel4_capdl_initializer_expected_untypeds_list_num_entries")
            .is_ok()
        {
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

    pub fn set_phys_base(&mut self, phys_base: u64) {
        self.phys_base = Some(phys_base);
    }
}
