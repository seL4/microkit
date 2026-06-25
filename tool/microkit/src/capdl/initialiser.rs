//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::ops::Range;

use rkyv::util::AlignedVec;

use crate::elf::{ElfProgramHeader64, ElfSegmentData, PF_R, PHENT_TYPE_LOADABLE, PHENT_TYPE_PHDR};
use crate::util::{round_up, struct_to_bytes};
use crate::{elf::ElfFile, sel4::PageSize};
use crate::{serialise_ut, UntypedObject};

// Page size used for allocating the spec and embedded frames segments.
pub const INITIALISER_GRANULE_SIZE: PageSize = PageSize::Small;

// Magic numbers for the initialiser to identify the data type.
// See rust-sel4 crates/sel4-phdrs/constants/src/lib.rs
const PT_SEL4_CAPDL_SPEC: u32 = 0x64c3_4003;
const PT_SEL4_CAPDL_FRAME_DATA: u32 = 0x64c3_4004;

pub struct CapDLInitialiserSpecMetadata {
    pub spec_size: u64,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

pub struct CapDLInitialiser {
    pub elf: ElfFile,
    pub phys_base: Option<u64>,
    pub spec_metadata: Option<CapDLInitialiserSpecMetadata>,
    /// Log level of initialiser printing in debug mode.
    pub log_level: LogLevel,
}

impl CapDLInitialiser {
    pub fn new(elf: ElfFile) -> CapDLInitialiser {
        CapDLInitialiser {
            elf,
            phys_base: None,
            spec_metadata: None,
            log_level: LogLevel::Info,
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

        // Follow implementation in rust-sel4: crates/sel4-capdl-initializer/add-spec/src/lib.rs
        let spec_vaddr = self.elf.next_vaddr(INITIALISER_GRANULE_SIZE);
        let spec_size = spec_payload.len() as u64;
        self.elf.add_segment(
            true,
            false,
            false,
            spec_vaddr,
            ElfSegmentData::RealData(spec_payload.into()),
            Some(PT_SEL4_CAPDL_SPEC),
        );

        let embedded_frame_data_vaddr = self.elf.next_vaddr(INITIALISER_GRANULE_SIZE);
        self.elf.add_segment(
            true,
            false,
            false,
            embedded_frame_data_vaddr,
            ElfSegmentData::RealData(embedded_frame_data),
            Some(PT_SEL4_CAPDL_FRAME_DATA),
        );

        // Now make the program headers table and inject it into the ELF as the initialiser look at
        // it to figure out its virtual address bound, spec location in memory etc.
        // It would have been nicer if we can perform this step in ElfFile::reserialise() but
        // that function is only relevant for x86.
        let phdrs_table_vaddr = self.elf.next_vaddr(INITIALISER_GRANULE_SIZE);
        let phdrs_table = self.elf.phdrs_table_serialised();
        // Accounts for a PHENT_TYPE_PHDR meta phdr + PHENT_TYPE_LOADABLE phdr when we eventually inject
        // the table as a segment.
        let expected_phnum = phdrs_table.len() + 2;
        let expected_phdrs_table_size_bytes = size_of::<ElfProgramHeader64>() * expected_phnum;

        let mut phdrs_table_bytes = vec![];
        phdrs_table.iter().for_each(|phdr| {
            phdrs_table_bytes.extend(unsafe { struct_to_bytes(&phdr.0) });
        });

        // Simulate what happens in ElfFile::add_segment() to derive the final program headers table.
        // We do this due to a chicken and egg problem, the real program headers table won't be finalised
        // until we add it as a segment. But we can't add it as a segment until we finalise it.
        phdrs_table_bytes.extend(unsafe {
            struct_to_bytes(&ElfProgramHeader64 {
                type_: PHENT_TYPE_PHDR,
                flags: PF_R,
                offset: 0,
                vaddr: phdrs_table_vaddr,
                paddr: phdrs_table_vaddr,
                filesz: expected_phdrs_table_size_bytes as u64,
                memsz: expected_phdrs_table_size_bytes as u64,
                align: 0,
            })
        });
        phdrs_table_bytes.extend(unsafe {
            struct_to_bytes(&ElfProgramHeader64 {
                type_: PHENT_TYPE_LOADABLE,
                flags: PF_R,
                offset: 0,
                vaddr: phdrs_table_vaddr,
                paddr: phdrs_table_vaddr,
                filesz: expected_phdrs_table_size_bytes as u64,
                memsz: expected_phdrs_table_size_bytes as u64,
                align: 0,
            })
        });

        self.elf.add_segment(
            true,
            false,
            false,
            phdrs_table_vaddr,
            ElfSegmentData::RealData(phdrs_table_bytes),
            Some(PHENT_TYPE_PHDR),
        );

        self.elf
            .write_symbol(
                "sel4_phdrs_patched__vaddr",
                &phdrs_table_vaddr.to_le_bytes(),
            )
            .unwrap();

        self.elf
            .write_symbol(
                "sel4_phdrs_patched__phnum",
                &(expected_phnum as u16).to_le_bytes(),
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

        // This feature is currently not in mainline rust-seL4, keep it around for potential
        // debugging purposes.
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
