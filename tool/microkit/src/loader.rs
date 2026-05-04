//
// Copyright 2024, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//
use crate::elf::{ElfFile, ElfSegmentData};
use crate::sel4::{Arch, Config};
use crate::uimage::uimage_serialise;
use crate::util::{mb, round_up, struct_to_bytes};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::ops::Range;
use std::path::Path;

macro_rules! grab_symbol {
    ($elf: expr, $symbol_name: literal) => {
        $elf.find_symbol($symbol_name)
            .expect(concat!("Could not find '", $symbol_name, "' symbol"))
    };
}

const PAGE_TABLE_SIZE: usize = 4096;

pub mod aarch64 {
    //! For AArch64, our page tables can be both Stage 2 (if we run in EL2) or Stage
    //! 1 (if we run in EL1, for non-hyp seL4). Generally, most attributes of the
    //! page tables for the features we use are compatible in the layouts between
    //! the Stage 2 descriptors and the Stage 1 descriptors. When modifying values
    //! of the descriptors, please ensure that the values are valid for both stages
    //! of the translation scheme.
    use crate::util::mask;

    pub const LVL0_BITS: u64 = 9;
    pub const LVL1_BITS: u64 = 9;
    pub const LVL2_BITS: u64 = 9;

    pub fn lvl0_index(addr: u64) -> usize {
        let idx = (addr >> (BLOCK_BITS_2MB + LVL2_BITS + LVL1_BITS)) & mask(LVL0_BITS);
        idx as usize
    }

    pub fn lvl1_index(addr: u64) -> usize {
        let idx = (addr >> (BLOCK_BITS_2MB + LVL2_BITS)) & mask(LVL1_BITS);
        idx as usize
    }

    pub fn lvl2_index(addr: u64) -> usize {
        let idx = (addr >> (BLOCK_BITS_2MB)) & mask(LVL2_BITS);
        idx as usize
    }

    /// The one main difference between the Stage 1 and Stage 2 translation
    /// tables is that bit[4:2] contains AttrIndex[2:0] (Stage 1) or
    /// MemAttr[3:0] (Stage 2).
    /// For Stage 1, the AttrIndex depends on our configured value of `MAIR_EL1`
    /// done in util64.S.
    /// For Stage 2, the values are fixed in Table D8-96 "Stage 2 MemAttr[3:0] encoding"
    #[derive(Copy, Clone)]
    pub enum MemAttr {
        Stage1 { attr_index: u64 },
        Stage2 { mem_attr: u64 },
    }

    impl MemAttr {
        pub fn value(&self) -> u64 {
            match *self {
                MemAttr::Stage1 { attr_index } => attr_index,
                MemAttr::Stage2 { mem_attr } => mem_attr,
            }
        }
        pub fn is_normal_cacheable(&self) -> bool {
            match *self {
                MemAttr::Stage1 { attr_index } => attr_index == s1_mair_attr_index::MT_NORMAL,
                MemAttr::Stage2 { mem_attr } => mem_attr == s2_mem_attr::NORMAL_INNER_WBC_OUTER_WBC,
            }
        }
    }

    /// These match those in util64.S configured in the MAIR_EL1 register,
    /// which also needs to match the values that seL4 uses.
    #[allow(non_upper_case_globals, reason = "matching ARM naming convention")]
    pub mod s1_mair_attr_index {
        pub const MT_DEVICE_nGnRnE: u64 = 0b000;
        pub const MT_DEVICE_nGnRE: u64 = 0b001;
        pub const MT_DEVICE_GRE: u64 = 0b010;
        pub const MT_NORMAL_NC: u64 = 0b011;
        pub const MT_NORMAL: u64 = 0b100;
    }

    /// See fixed values in Table D8-96 "Stage 2 MemAttr[3:0] encoding"
    #[allow(non_upper_case_globals, reason = "matching ARM naming convention")]
    pub mod s2_mem_attr {
        pub const DEVICE_nGnRnE: u64 = 0b0000;
        pub const DEVICE_nGnRE: u64 = 0b0001;
        pub const DEVICE_nGRE: u64 = 0b0010;
        pub const DEVICE_GRE: u64 = 0b0011;

        pub const NORMAL_INNER_NC_OUTER_NC: u64 = 0b0101;
        pub const NORMAL_INNER_WTC_OUTER_NC: u64 = 0b0110;
        pub const NORMAL_INNER_WBC_OUTER_NC: u64 = 0b0111;

        pub const NORMAL_INNER_NC_OUTER_WTC: u64 = 0b1001;
        pub const NORMAL_INNER_WTC_OUTER_WTC: u64 = 0b1010;
        pub const NORMAL_INNER_WBC_OUTER_WTC: u64 = 0b1011;

        pub const NORMAL_INNER_NC_OUTER_WBC: u64 = 0b1101;
        pub const NORMAL_INNER_WTC_OUTER_WBC: u64 = 0b1110;
        pub const NORMAL_INNER_WBC_OUTER_WBC: u64 = 0b1111;
    }

    pub mod descriptor_type {
        //! The translation table descriptor formats, as per §D8.3 "Translation
        //! table descriptor formats" of ARM DDI 0487 L.b. Specifically,
        //! as per "Table D8-48 Determination of descriptor type"

        /// Descriptor type: Table. Condition is lookup level != 3.
        pub const TABLE: u64 = 0b11;
        /// Descriptor type: Page. Condition is lookup level == 3.
        pub const PAGE: u64 = 0b11;
        /// Descriptor type: Block. Condition is lookup level != 3.
        pub const BLOCK: u64 = 0b01;
        /// Descriptor type: Invalid. Strictly speaking bit[1] does not matter.
        pub const INVALID: u64 = 0b00;
    }

    pub mod shareability_attributes {
        //! Per §D8.6.2 "Stage 1 Shareability attributes", these contain the
        //! shareability attributes of the descriptor OA for normal-cacheable
        //! memory. These values are the same as in §D8.6.7 "Stage 2 Shareability
        //! attributes", so we can use the same for both Stage 1 / Stage 2.

        /// Non-shareable
        pub const NON_SHAREABLE: u64 = 0b00;
        /// Outer-shareable
        pub const OUTER_SHAREABLE: u64 = 0b10;
        /// Inner-shareable
        pub const INNER_SHAREABLE: u64 = 0b11;
    }

    /// Per "Figure D8-14 VMSAv8-64 Block descriptor formats" of ARM DDI0487L.b,
    /// subfigure "4KB, 16KB, and 64KB granules, 48-bit OA", the Output address
    /// is bits [47:n], and:
    ///
    /// > For the 4KB granule size, the level 1 descriptor n is 30,
    /// > and the level 2 descriptor n is 21.
    pub const BLOCK_BITS_1GB: u64 = 30;

    /// Per "Figure D8-14 VMSAv8-64 Block descriptor formats" of ARM DDI0487L.b,
    /// subfigure "4KB, 16KB, and 64KB granules, 48-bit OA", the Output address
    /// is bits [47:n], and:
    ///
    /// > For the 4KB granule size, the level 1 descriptor n is 30,
    /// > and the level 2 descriptor n is 21.
    pub const BLOCK_BITS_2MB: u64 = 21;

    /// Per "Table D8-52 Stage 1 VMSAv8-64 Block and Page descriptor fields" and
    /// "Figure D8-14 VMSAv8-64 Block descriptor formats" of ARM DDI0487L.b;
    /// specifically subfigure "4KB, 16KB, and 64KB granules, 48-bit OA"
    /// Also "Table D8-53 Stage 2 VMSAv8-64 Block and Page descriptor fields"
    /// for the Stage 2 attributes.
    pub fn block_descriptor(level: usize, addr: u64, mem_attr: MemAttr) -> u64 {
        // Per Table D8-48, Condition for descriptor_type::BLOCK is level != 3.
        assert!(level != 3);

        let upper_attributes: u64 = 0;

        let shareability = if mem_attr.is_normal_cacheable() {
            // Match what the seL4 kernel uses for its page tables
            shareability_attributes::INNER_SHAREABLE
        } else {
            // Per $R_{PYFVQ}$ (Stage 1) and $R_{RYHCTP}$ (Stage 2):
            // > If a region is mapped as Device memory or Normal Non-cacheable
            // > memory after all enabled translation stages, then the region
            // > has an effective Shareability attribute of Outer Shareable.
            //
            // We override the value we place in here to OUTER_SHAREABLE to match
            // how the hardware behaves. This is not necessary but for clarity.
            shareability_attributes::OUTER_SHAREABLE
        };

        // AP[2:1], which we set as 0b00 for read/write access:
        //   stage 1: 0b00 is {PrivRead, PrivWrite} and we are EL1
        //   stage 2: 0b00 is RW for EL2 and no perms for EL1.
        const AP_KERNEL_RW: u64 = 0b00;

        // bit[11] is the not global (nG) field, we leave as 0 (global).
        //         In Stage2 it is RES0.
        // bit[10] is the access flag; depending on FEAT_HAFDBS, when software
        //         manages the AF memory accesses to the page/block when AF=0
        //         raise an Access Fault; when hardware manages the AF it will
        //         become 1.
        // bit[9:8] is SH[1:0] containing stage 1 shareability attributes
        // bit[7:6] contains AP[2:1]
        // bit[5] is RES0
        // bit[4:2] contains AttrIndex (Stage 1) or MemAttr (Stage 2)
        let lower_attributes: u64 =
            (1 << 10) | (AP_KERNEL_RW << 6) | (shareability << 8) | (mem_attr.value() << 2);

        // bits[47:n]
        let output_address: u64 = addr
            & !mask(match level {
                1 => BLOCK_BITS_1GB,
                2 => BLOCK_BITS_2MB,
                _ => panic!("unsupported level {level} for block descriptor"),
            });

        // address must not have bits above 47 set.
        assert!(addr & mask(48) == addr);

        // bits[63:50] describing the "Upper attributes" are left at 0.
        // bits[49:48] are RES0
        // bits[47:n] contain the Output address
        // bits[n-1:12] are RES0
        // bits[11:2] contain the "Lower attributes"
        // bits[1:0] contains the descriptor type
        upper_attributes | output_address | lower_attributes | descriptor_type::BLOCK
    }

    /// Per "Table D8-52 Stage 1 VMSAv8-64 Block and Page descriptor fields" and
    /// "Figure D8-15 VMSAv8-64 Page descriptor formats" of ARM DDI0487L.b;
    /// specifically subfigure "4KB granule 48-bit OA".
    /// Also "Table D8-53 Stage 2 VMSAv8-64 Block and Page descriptor fields"
    /// for the Stage 2 attributes.
    pub fn page_descriptor(addr: u64, mem_attr: MemAttr) -> u64 {
        // The main difference between a page descriptor and block descriptor
        // is in the size of the output address (OA) and in the descriptor type.

        let upper_attributes: u64 = 0;

        let shareability = if mem_attr.is_normal_cacheable() {
            // Match what the seL4 kernel uses for its page tables
            shareability_attributes::INNER_SHAREABLE
        } else {
            // Per $R_{PYFVQ}$:
            // > If a region is mapped as Device memory or Normal Non-cacheable
            // > memory after all enabled translation stages, then the region
            // > has an effective Shareability attribute of Outer Shareable.
            // We override the value we place in here to OUTER_SHAREABLE to match
            // how the hardware behaves.
            shareability_attributes::OUTER_SHAREABLE
        };

        // AP[2:1], which we set as 0b00 for read/write access:
        //   stage 1: 0b00 is {PrivRead, PrivWrite} and we are EL1
        //   stage 2: 0b00 is RW for EL2 and no perms for EL1.
        const AP_KERNEL_RW: u64 = 0b00;

        // bit[11] is the not global (nG) field, we leave as 0 (global).
        //         In Stage2 it is RES0
        // bit[10] is the access flag; depending on FEAT_HAFDBS, when software
        //         manages the AF memory accesses to the page/block when AF=0
        //         raise an Access Fault; when hardware manages the AF it will
        //         become 1.
        // bit[9:8] is SH[1:0] containing stage 1 shareability attributes
        // bit[7:6] contains AP[2:1]
        // bit[5] is RES0
        // bit[4:2] contains AttrIndex (Stage 1) or MemAttr (Stage 2)
        let lower_attributes: u64 =
            (1 << 10) | (AP_KERNEL_RW << 6) | (shareability << 8) | (mem_attr.value() << 2);

        // bits[47:12]
        let output_address: u64 = addr & !mask(12);

        // address must not have bits above 47 set.
        assert!(addr & mask(48) == addr);

        // bits[63:50] describing the "Upper attributes" are left at 0.
        // bits[49:48] are RES0
        // bits[47:12] contain the Output address
        // bits[11:2] contain the "Lower attributes"
        // bits[1:0] contains the descriptor type
        upper_attributes | output_address | lower_attributes | descriptor_type::PAGE
    }

    /// Per "Table D8-50 Stage 1 VMSAv8-64 Table descriptor fields" and
    /// "Figure D8-12 VMSAv8-64 Table descriptor formats" of ARM DDI0487L.b;
    /// specifically subfigure "4KB, 16KB, and 64KB granules, 48-bit OA"
    pub fn table_descriptor(addr: u64) -> u64 {
        // Per Table D8-48, Condition for descriptor_type::TABLE is level != 3.

        // We don't set any of these attributes, most are hardware-feature conditional
        let attributes: u64 = 0;

        // address must not have bits above 47 or below 12 set
        assert!(addr & mask(12) == 0x0);
        assert!(addr & mask(48) == addr);

        let next_level_table_address = addr;

        // bits[63:59] are "Attributes"
        // bits[58:51] are ignored
        // bits[50:48] are RES0
        // bits[47:m] is the next-level table address
        //  note: here m=12 for 4KB granule
        // bits[m-1:12] are RES0
        //  so this doesn't exist for 4KB granule
        // bits[11:2] are ignored
        // bits[1:0] contain the descriptor type
        attributes | next_level_table_address | descriptor_type::TABLE
    }
}

mod riscv64 {
    pub(crate) const BLOCK_BITS_2MB: u64 = 21;
    pub(crate) const PAGE_BITS_4K: u64 = 12;

    pub(crate) const PAGE_TABLE_INDEX_BITS: u64 = 9;
    pub(crate) const PAGE_SHIFT: u64 = 12;
    /// This sets the page table entry bits: D,A,X,W,R.
    pub(crate) const PTE_TYPE_BITS: u64 = 0b11001110;
    // TODO: where does this come from?
    pub(crate) const PTE_TYPE_TABLE: u64 = 0;
    pub(crate) const PTE_TYPE_VALID: u64 = 1;

    pub(crate) const PTE_PPN0_SHIFT: u64 = 10;

    /// Due to RISC-V having various virtual memory setups, we have this generic function to
    /// figure out the page-table index given the total number of page table levels for the
    /// platform and which level we are currently looking at.
    pub fn pt_index(pt_levels: usize, addr: u64, level: usize) -> usize {
        let pt_index_bits = PAGE_TABLE_INDEX_BITS * (pt_levels - level) as u64;
        let idx = (addr >> (pt_index_bits + PAGE_SHIFT)) % 512;

        idx as usize
    }

    /// Generate physical page number given an address
    pub fn pte_ppn(addr: u64) -> u64 {
        (addr >> PAGE_SHIFT) << PTE_PPN0_SHIFT
    }

    pub fn pte_next(addr: u64) -> u64 {
        pte_ppn(addr) | PTE_TYPE_TABLE | PTE_TYPE_VALID
    }

    pub fn pte_leaf(addr: u64) -> u64 {
        pte_ppn(addr) | PTE_TYPE_BITS | PTE_TYPE_VALID
    }
}

/// Checks that each region in the given list does not overlap with any other region.
/// Panics upon finding an overlapping region
fn check_non_overlapping(regions: &Vec<(u64, &[u8])>) {
    let mut checked: Vec<(u64, u64)> = Vec::new();
    for (base, data) in regions {
        let end = base + data.len() as u64;
        // Check that this does not overlap with any checked regions
        for (b, e) in &checked {
            if !(end <= *b || *base >= *e) {
                panic!("Overlapping regions: [{base:x}..{end:x}) overlaps [{b:x}..{e:x})");
            }
        }

        checked.push((*base, end));
    }
}

#[repr(C)]
struct LoaderRegion64 {
    load_addr: u64,
    size: u64,
    offset: u64,
    r#type: u64,
}

#[repr(C)]
struct LoaderHeader64 {
    magic: u64,
    size: u64,
    kernel_entry: u64,
    ui_p_reg_start: u64,
    ui_p_reg_end: u64,
    pv_offset: u64,
    v_entry: u64,
    num_regions: u64,
}

pub struct Loader<'a> {
    arch: Arch,
    loader_image: Vec<u8>,
    header: LoaderHeader64,
    region_metadata: Vec<LoaderRegion64>,
    regions: Vec<(u64, &'a [u8])>,
    word_size: usize,
    elf_machine: u16,
    entry: u64,
}

impl<'a> Loader<'a> {
    pub fn new(
        config: &Config,
        loader_elf_path: &Path,
        kernel_elf: &'a ElfFile,
        initial_task_elf: &'a ElfFile,
        initial_task_phy_base: u64,
        initial_task_vaddr_range: &Range<u64>,
    ) -> Loader<'a> {
        if config.arch == Arch::X86_64 {
            unreachable!("internal error: x86_64 does not support creating a loader image");
        }

        let loader_elf = ElfFile::from_path(loader_elf_path).unwrap_or_else(|e| {
            eprintln!(
                "ERROR: failed to parse loader ELF ({}): {}",
                loader_elf_path.display(),
                e
            );
            std::process::exit(1);
        });
        let sz = loader_elf.word_size;
        let magic = match sz {
            32 => 0x5e14dead,
            64 => 0x5e14dead14de5ead,
            _ => panic!(
                "Internal error: unexpected ELF word size: {} from '{}'",
                sz,
                loader_elf_path.display()
            ),
        };

        let mut regions: Vec<(u64, &[u8])> = Vec::new();

        let mut kernel_first_vaddr = None;
        let mut kernel_last_vaddr = None;
        let mut kernel_first_paddr = None;
        let mut kernel_p_v_offset = None;

        for segment in &kernel_elf.segments {
            if segment.loadable {
                if kernel_first_vaddr.is_none() || segment.virt_addr < kernel_first_vaddr.unwrap() {
                    kernel_first_vaddr = Some(segment.virt_addr);
                }

                if kernel_last_vaddr.is_none()
                    || segment.virt_addr + segment.mem_size() > kernel_last_vaddr.unwrap()
                {
                    kernel_last_vaddr =
                        Some(round_up(segment.virt_addr + segment.mem_size(), mb(2)));
                }

                if kernel_first_paddr.is_none() || segment.phys_addr < kernel_first_paddr.unwrap() {
                    kernel_first_paddr = Some(segment.phys_addr);
                }

                if let Some(p_v_offset) = kernel_p_v_offset {
                    if p_v_offset != segment.virt_addr - segment.phys_addr {
                        panic!("Kernel does not have a consistent physical to virtual offset");
                    }
                } else {
                    kernel_p_v_offset = Some(segment.virt_addr - segment.phys_addr);
                }

                regions.push((segment.phys_addr, segment.data().as_slice()));
            }
        }

        // We support an initial task ELF with multiple segments. This is implemented by amalgamating all the segments
        // into 1 segment, so if your segments are sparse, a lot of memory will be wasted.
        let initial_task_segments = initial_task_elf.loadable_segments();

        // Compute an available physical memory segment large enough to house the initial task (CapDL initialiser with spec)
        // that is after the kernel window.
        let inittask_v_entry = initial_task_elf.entry;

        for segment in initial_task_segments.iter() {
            if segment.mem_size() > 0 {
                let segment_paddr =
                    initial_task_phy_base + (segment.virt_addr - initial_task_vaddr_range.start);
                regions.push((segment_paddr, segment.data()));
            }
        }

        let Some(kernel_first_vaddr) = kernel_first_vaddr else {
            panic!("INTERNAL: could not determine kernel_first_vaddr");
        };

        let Some(kernel_first_paddr) = kernel_first_paddr else {
            panic!("INTERNAL: could not determine kernel_first_paddr");
        };

        let pagetable_vars = match config.arch {
            Arch::Aarch64 => Loader::aarch64_setup_pagetables(
                config,
                &loader_elf,
                kernel_first_vaddr,
                kernel_first_paddr,
            ),
            Arch::Riscv64 => Loader::riscv64_setup_pagetables(
                config,
                &loader_elf,
                kernel_first_vaddr,
                kernel_first_paddr,
            ),
            Arch::X86_64 => unreachable!("x86_64 does not support creating a loader image"),
        };

        let image_segment = loader_elf
            .segments
            .into_iter()
            .find(|segment| segment.loadable)
            .expect("Did not find loadable segment");
        let image_vaddr = image_segment.virt_addr;
        // We have to clone here as the image executable is part of this function return object,
        // and the loader ELF is deserialised in this scope, so its lifetime will be shorter than
        // the return object.
        let mut loader_image = image_segment.data().clone();

        if image_vaddr != loader_elf.entry {
            panic!("The loader entry point must be the first byte in the image");
        }

        for (var_addr, var_size, var_data) in pagetable_vars {
            let offset = var_addr - image_vaddr;
            assert!(var_size == var_data.len() as u64);
            assert!(offset > 0);
            assert!(offset <= loader_image.len() as u64);
            loader_image[offset as usize..(offset + var_size) as usize].copy_from_slice(&var_data);
        }

        let kernel_entry = kernel_elf.entry;

        // initial task virt + pv_offset == initial task physical, so
        // pv_offset == initial task physical - initial task virt
        let pv_offset = initial_task_phy_base.wrapping_sub(initial_task_vaddr_range.start);

        let ui_p_reg_start = initial_task_phy_base;
        let ui_p_reg_end =
            ui_p_reg_start + (initial_task_vaddr_range.end - initial_task_vaddr_range.start);
        assert!(ui_p_reg_end > ui_p_reg_start);

        // This clone isn't too bad as it is just a Vec<(u64, &[u8])>
        let mut all_regions_with_loader = regions.clone();
        all_regions_with_loader.push((image_vaddr, &loader_image));
        check_non_overlapping(&all_regions_with_loader);

        let mut region_metadata = Vec::new();
        let mut offset: u64 = 0;
        for (addr, data) in &regions {
            region_metadata.push(LoaderRegion64 {
                load_addr: *addr,
                size: data.len() as u64,
                offset,
                r#type: 1,
            });
            offset += data.len() as u64;
        }

        let size = std::mem::size_of::<LoaderHeader64>() as u64
            + region_metadata.iter().fold(0_u64, |acc, x| {
                acc + x.size + std::mem::size_of::<LoaderRegion64>() as u64
            });

        let header = LoaderHeader64 {
            magic,
            size,
            kernel_entry,
            ui_p_reg_start,
            ui_p_reg_end,
            pv_offset,
            v_entry: inittask_v_entry,
            num_regions: regions.len() as u64,
        };

        Loader {
            arch: config.arch,
            loader_image,
            header,
            region_metadata,
            regions,
            word_size: kernel_elf.word_size,
            elf_machine: kernel_elf.machine,
            entry: loader_elf.entry,
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        // First copy image data, which includes the Microkit bootloader's code, etc
        bytes.extend_from_slice(&self.loader_image);
        // Then we copy the loader metadata (known as the 'header')
        bytes.extend_from_slice(unsafe { struct_to_bytes(&self.header) });
        // For each region, we need to copy the region metadata as well
        for region in &self.region_metadata {
            let region_metadata_bytes = unsafe { struct_to_bytes(region) };
            bytes.extend_from_slice(region_metadata_bytes);
        }
        // Now we can copy all the region data
        for (_, data) in &self.regions {
            bytes.extend_from_slice(data);
        }

        bytes
    }

    pub fn write_image(&self, path: &Path) {
        let loader_file = match File::create(path) {
            Ok(file) => file,
            Err(e) => panic!("Could not create '{}': {}", path.display(), e),
        };

        let mut loader_buf = BufWriter::new(loader_file);

        // First write out all the image data
        loader_buf
            .write_all(&self.to_bytes())
            .expect("Failed to write image data to loader");

        loader_buf.flush().unwrap();
    }

    fn convert_to_elf(&self, path: &Path) -> ElfFile {
        let mut loader_elf = ElfFile::new(
            path.to_path_buf(),
            self.word_size,
            self.entry,
            self.elf_machine,
        );

        loader_elf.add_segment(
            true,
            true,
            true,
            self.entry,
            ElfSegmentData::RealData(self.to_bytes()),
        );

        loader_elf
    }

    pub fn write_elf(&self, path: &Path) {
        let loader_elf = self.convert_to_elf(path);

        match loader_elf.reserialise(path) {
            Ok(_) => {}
            Err(e) => panic!("Could not create '{}': {}", path.display(), e),
        }
    }

    pub fn write_uimage(&self, path: &Path) {
        let executable_payload = self.to_bytes();
        let entry_32: u32 = match <u64 as TryInto<u32>>::try_into(self.entry) {
            Ok(entry_32) => entry_32,
            Err(_) => panic!(
                "Could not create '{}': Loader link address 0x{:x} cannot be above 4G for uImage.",
                path.display(),
                self.entry
            ),
        };

        match uimage_serialise(&self.arch, entry_32, executable_payload, path) {
            Ok(_) => {}
            Err(e) => panic!("Could not create '{}': {}", path.display(), e),
        }
    }

    fn riscv64_setup_pagetables(
        config: &Config,
        elf: &ElfFile,
        first_vaddr: u64,
        first_paddr: u64,
    ) -> Vec<(u64, u64, [u8; PAGE_TABLE_SIZE])> {
        let (text_addr, _) = grab_symbol!(elf, "_text");
        let (boot_lvl1_pt_addr, boot_lvl1_pt_size) = grab_symbol!(elf, "boot_lvl1_pt");
        let (boot_lvl2_pt_addr, boot_lvl2_pt_size) = grab_symbol!(elf, "boot_lvl2_pt");
        let (boot_lvl3_pt_addr, boot_lvl3_pt_size) = grab_symbol!(elf, "boot_lvl3_pt");
        let (boot_lvl2_pt_loader_addr, boot_lvl2_pt_loader_size) =
            grab_symbol!(elf, "boot_lvl2_pt_loader");

        // We map the loader using 2MB pages, so make sure the base is actually aligned.
        assert!(text_addr.is_multiple_of(1 << riscv64::BLOCK_BITS_2MB));

        let num_pt_levels = config.riscv_pt_levels.unwrap().levels();

        let mut boot_lvl1_pt: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];
        {
            let text_index_lvl1 = riscv64::pt_index(num_pt_levels, text_addr, 1);
            let pt_entry = riscv64::pte_next(boot_lvl2_pt_loader_addr);
            let start = 8 * text_index_lvl1;
            let end = start + 8;
            boot_lvl1_pt[start..end].copy_from_slice(&pt_entry.to_le_bytes());
        }

        let mut boot_lvl2_pt_loader: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];
        {
            let text_index_lvl2 = riscv64::pt_index(num_pt_levels, text_addr, 2);
            for (page, i) in (text_index_lvl2..512).enumerate() {
                let start = 8 * i;
                let end = start + 8;
                let addr = text_addr + ((page as u64) << riscv64::BLOCK_BITS_2MB);
                let pt_entry = riscv64::pte_leaf(addr);
                boot_lvl2_pt_loader[start..end].copy_from_slice(&pt_entry.to_le_bytes());
            }
        }

        {
            let index = riscv64::pt_index(num_pt_levels, first_vaddr, 1);
            let start = 8 * index;
            let end = start + 8;
            boot_lvl1_pt[start..end]
                .copy_from_slice(&riscv64::pte_next(boot_lvl2_pt_addr).to_le_bytes());
        }

        let mut boot_lvl3_pt: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];
        let mut boot_lvl2_pt: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];
        {
            let mut index_lvl2 = riscv64::pt_index(num_pt_levels, first_vaddr, 2);
            if !first_vaddr.is_multiple_of(1 << riscv64::BLOCK_BITS_2MB) {
                let index_lvl3 = riscv64::pt_index(num_pt_levels, first_vaddr, 3);
                for (page, i) in (index_lvl3..512).enumerate() {
                    let start = 8 * i;
                    let end = start + 8;
                    let addr = first_paddr + ((page as u64) << riscv64::PAGE_BITS_4K);
                    assert!(addr.is_multiple_of(1 << riscv64::PAGE_BITS_4K));
                    let pt_entry = riscv64::pte_leaf(addr);
                    boot_lvl3_pt[start..end].copy_from_slice(&pt_entry.to_le_bytes());
                }
                let start = 8 * index_lvl2;
                let end = start + 8;
                let lvl3_pt_entry = riscv64::pte_next(boot_lvl3_pt_addr);
                assert!(boot_lvl3_pt_addr.is_multiple_of(1 << riscv64::PAGE_BITS_4K));
                boot_lvl2_pt[start..end].copy_from_slice(&lvl3_pt_entry.to_le_bytes());
                index_lvl2 += 1;
            }
            let first_paddr_aligned = round_up(first_paddr, 1 << riscv64::BLOCK_BITS_2MB);
            for (page, i) in (index_lvl2..512).enumerate() {
                let start = 8 * i;
                let end = start + 8;
                let addr = first_paddr_aligned + ((page as u64) << riscv64::BLOCK_BITS_2MB);
                assert!(addr.is_multiple_of(1 << riscv64::BLOCK_BITS_2MB));
                let pt_entry = riscv64::pte_leaf(addr);
                boot_lvl2_pt[start..end].copy_from_slice(&pt_entry.to_le_bytes());
            }
        }

        vec![
            (boot_lvl1_pt_addr, boot_lvl1_pt_size, boot_lvl1_pt),
            (boot_lvl2_pt_addr, boot_lvl2_pt_size, boot_lvl2_pt),
            (boot_lvl3_pt_addr, boot_lvl3_pt_size, boot_lvl3_pt),
            (
                boot_lvl2_pt_loader_addr,
                boot_lvl2_pt_loader_size,
                boot_lvl2_pt_loader,
            ),
        ]
    }

    /// AArch64 loader page tables have two variations:
    ///  - Loader in EL2, then Stage 2 translations in use, so we have the
    ///    singular TTBR0_EL2 register containing the Level 0 table;
    ///    this allows virtual address in the range [0,2^48).
    ///  - Loader in EL1, then Stage 1 translations are in use, so we have both
    ///    the TTBR0_EL1 (covering vaddr in range [0,2^48)) and TTBR1_EL2 (
    ///    (covering vaddr in the range [2^64-2^48,2^64)), and containing
    ///    the "Level 0 Lower" page table, and "Level 0 Upper" page table
    ///    physical addresses respectively.
    ///
    /// Thus, for EL2 loader, the singular Level 0 page table contains the table
    /// descriptors for the "Level 1 Upper" and "Level 1 Lower" page tables.
    /// For the EL1 loader, we instead have two Level 0 page tables, and
    /// "Level 0 Lower" contains the "Level 1 Lower" descriptor, and "Level 0
    /// Upper" contains the "Level 1 Upper" descriptor.
    /// Otherwise, the page tables layout from Level 1 downwards are identical
    /// (but not necessarily the layout within the page/table/block descriptors).
    ///
    /// ```txt
    ///          (256 TiB)
    ///   512 +-- Level 0 --+ 2^48
    ///       |             |
    ///       |   (empty)   |
    ///       |             |
    ///   k+1 +-------------+                 (512 GiB)
    ///       | Level 1 Upr | ---------->  +-- Level 1 --+
    ///     k +-------------+              |             |
    ///       |             |              |   (empty)   |
    ///       |             |              |             |
    ///       |             |         l+1  +-------------+                 (1 GiB)
    ///       |             |              | Level 2 Upr | ----------> +-- Level 2 --+             +-------------+
    ///       |             |           l  +-------------+             |             | ----------> | 2 MiB block |
    ///       |             |              |             |         511 |-------------|             +-------------+
    ///       |             |              |   (empty)   |             |             | ----------> | 2 MiB block |
    ///       |             |              |             |         510 |-------------|             +-------------+
    ///       |             |              +-------------+             |             | ----------> | 2 MiB block |
    ///       |             |                                          |-------------|             +-------------+
    ///       |   (empty)   |                          Kernel Regions       (...)         (...)         (...)
    ///       |             |                                          |-------------|             +-------------+
    ///       |             |                                          |             | ----------> | 2 MiB block |
    ///       |             |                                        m |-------------|             +-------------+ p
    ///       |             |                                          |             |
    ///       |             |                                          |   (empty)   |
    ///       |             |                                          |             |
    ///       |             |                                        0 +-------------+
    ///       |             |
    ///       |             |
    ///       |             |
    ///     1 +-------------+                 (512 GiB)
    ///       | Level 1 Lwr | ---------->  +-- Level 1 --+
    ///     0 +-------------+              |             |
    ///                                    |   (empty)   |
    ///                                    |             |
    ///                                u+1 +-------------+             +-------------+
    ///                                    |  uart_base  | ----------> | 1 GiB block |
    ///                                  u +-------------+             +-------------+
    ///                                    |             |
    ///                                    |   (empty)   |
    ///                                    |             |
    ///                               i+1  +-------------+                 (1 GiB)
    ///                                    | Level 2 Lwr | ----------> +-- Level 2 --+
    ///                                 i  +-------------+             |             |
    ///                                    |             |             |   (empty)   |
    ///                                    |   (empty)   |             |             |
    ///                                    |             |           t +-------------+             +-------------+
    ///                                    +-------------+             |             | ----------> | 2 MiB block |
    ///                                                                |-------------|             +-------------+
    ///                                                                |             | ----------> | 2 MiB block |
    ///                                                                |-------------|             +-------------+
    ///                                                Loader Regions       (...)         (...)         (...)
    ///                                                                |-------------|             +-------------+
    ///                                                                |             | ----------> | 2 MiB block |
    ///                                                                |-------------|             +-------------+
    ///                                                                |             | ----------> | 2 MiB block |
    ///                                                              s +-------------+             +-------------+
    ///                                                                |             |
    ///                                                                |   (empty)   |
    ///                                                                |             |
    ///                                                                +-------------+
    ///
    /// Where:
    ///      k = align_down(kernel_first_vaddr, 512GiB),
    ///      l = align_down(kernel_first_vaddr, 1GiB),
    ///      m = align_down(kernel_first_vaddr, 2MiB),
    ///      p = align_down(kernel_first_paddr, 2MiB),
    ///      u = align_down(uart_base, 1GiB),
    ///      i = align_down(loader_start_addr, 1GiB),
    ///      s = align_down(loader_start_addr, 2MiB),
    ///      t = align_up(loader_end_addr, 2MiB),
    /// ```
    ///
    fn aarch64_setup_pagetables(
        config: &Config,
        elf: &ElfFile,
        first_vaddr: u64,
        first_paddr: u64,
    ) -> Vec<(u64, u64, [u8; PAGE_TABLE_SIZE])> {
        let (boot_lvl1_lower_addr, boot_lvl1_lower_size) = grab_symbol!(elf, "boot_lvl1_lower");
        let (boot_lvl1_upper_addr, boot_lvl1_upper_size) = grab_symbol!(elf, "boot_lvl1_upper");
        let (boot_lvl2_upper_addr, boot_lvl2_upper_size) = grab_symbol!(elf, "boot_lvl2_upper");
        let (boot_lvl0_lower_addr, boot_lvl0_lower_size) = grab_symbol!(elf, "boot_lvl0_lower");
        let (boot_lvl0_upper_addr, boot_lvl0_upper_size) = grab_symbol!(elf, "boot_lvl0_upper");
        let (boot_lvl2_lower_addr, boot_lvl2_lower_size) = grab_symbol!(elf, "boot_lvl2_lower");

        let (loader_start_addr, _) = grab_symbol!(elf, "_loader_start");
        let (loader_end_addr, _) = grab_symbol!(elf, "_loader_end");

        // Stage 1 or 2 depends on hypervisor config.
        #[rustfmt::skip]
        let memattr_normal = if config.hypervisor {
            aarch64::MemAttr::Stage2 { mem_attr: aarch64::s2_mem_attr::NORMAL_INNER_WBC_OUTER_WBC }
        } else {
            aarch64::MemAttr::Stage1 { attr_index: aarch64::s1_mair_attr_index::MT_NORMAL }
        };
        #[rustfmt::skip]
        let memattr_device = if config.hypervisor {
            aarch64::MemAttr::Stage2 { mem_attr: aarch64::s2_mem_attr::DEVICE_nGnRnE }
        } else {
            aarch64::MemAttr::Stage1 { attr_index: aarch64::s1_mair_attr_index::MT_DEVICE_nGnRnE }
        };

        if aarch64::lvl1_index(loader_start_addr) != aarch64::lvl1_index(loader_end_addr) {
            panic!("We only map 1GiB, but loader paddr range covers multiple GiB");
        }

        let mut boot_lvl0_lower: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];
        {
            let pt_entry = aarch64::table_descriptor(boot_lvl1_lower_addr);
            boot_lvl0_lower[..8].copy_from_slice(&pt_entry.to_le_bytes());
        }

        let mut boot_lvl1_lower: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];

        // map optional UART MMIO in l1 1GB page, only available if CONFIG_PRINTING
        if let Ok((uart_addr, uart_addr_size)) = elf.find_symbol("uart_addr") {
            let data = elf
                .get_data(uart_addr, uart_addr_size)
                .expect("uart_addr not initialized");

            let uart_base = u64::from_le_bytes(data[0..8].try_into().unwrap());

            let lvl1_idx = aarch64::lvl1_index(uart_base);

            let pt_entry = aarch64::block_descriptor(1, uart_base, memattr_device);

            let start = 8 * lvl1_idx;
            let end = 8 * (lvl1_idx + 1);
            boot_lvl1_lower[start..end].copy_from_slice(&pt_entry.to_le_bytes());
        }

        let mut boot_lvl2_lower: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];

        // 1GB lvl1 Table entry
        let pt_entry = aarch64::table_descriptor(boot_lvl2_lower_addr);
        let lvl1_idx = aarch64::lvl1_index(loader_start_addr);
        let start = 8 * lvl1_idx;
        let end = 8 * (lvl1_idx + 1);
        boot_lvl1_lower[start..end].copy_from_slice(&pt_entry.to_le_bytes());

        // map the loader 1:1 access into 2MB lvl2 Block entries for a 4KB granule
        let lvl2_idx = aarch64::lvl2_index(loader_start_addr);
        for i in lvl2_idx..=aarch64::lvl2_index(loader_end_addr) {
            let entry_idx: u64 =
                ((i - aarch64::lvl2_index(loader_start_addr)) << aarch64::BLOCK_BITS_2MB) as u64;

            let pt_entry =
                aarch64::block_descriptor(2, loader_start_addr + entry_idx, memattr_device);

            let start = 8 * i;
            let end = 8 * (i + 1);
            boot_lvl2_lower[start..end].copy_from_slice(&pt_entry.to_le_bytes());
        }

        // TODO: this is a complete hack specific to BCM2711/Raspberry Pi 4B and
        // will be removed with patches that re-do this loader mapping code.
        if elf.find_symbol("cpus_release_addr").is_ok() {
            let lvl2_idx = aarch64::lvl2_index(0);
            // Make sure we don't override the loader mappings done above.
            assert!(aarch64::lvl2_index(loader_start_addr) != lvl2_idx);
            assert!(aarch64::lvl1_index(loader_start_addr) == aarch64::lvl1_index(0));

            let pt_entry = aarch64::block_descriptor(2, lvl2_idx as u64, memattr_device);

            let start = 8 * lvl2_idx;
            let end = 8 * (lvl2_idx + 1);
            boot_lvl2_lower[start..end].copy_from_slice(&pt_entry.to_le_bytes());
        }

        let boot_lvl0_upper: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];
        {
            let pt_entry = aarch64::table_descriptor(boot_lvl1_upper_addr);
            let idx = aarch64::lvl0_index(first_vaddr);
            boot_lvl0_lower[8 * idx..8 * (idx + 1)].copy_from_slice(&pt_entry.to_le_bytes());
        }

        let mut boot_lvl1_upper: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];
        {
            let pt_entry = aarch64::table_descriptor(boot_lvl2_upper_addr);
            let idx = aarch64::lvl1_index(first_vaddr);
            boot_lvl1_upper[8 * idx..8 * (idx + 1)].copy_from_slice(&pt_entry.to_le_bytes());
        }

        let mut boot_lvl2_upper: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];

        let lvl2_idx = aarch64::lvl2_index(first_vaddr);
        for i in lvl2_idx..512 {
            let entry_idx: u64 =
                ((i - aarch64::lvl2_index(first_vaddr)) << aarch64::BLOCK_BITS_2MB) as u64;

            let pt_entry = aarch64::block_descriptor(2, first_paddr + entry_idx, memattr_normal);

            let start = 8 * i;
            let end = 8 * (i + 1);
            boot_lvl2_upper[start..end].copy_from_slice(&pt_entry.to_le_bytes());
        }

        vec![
            (boot_lvl0_lower_addr, boot_lvl0_lower_size, boot_lvl0_lower),
            (boot_lvl1_lower_addr, boot_lvl1_lower_size, boot_lvl1_lower),
            (boot_lvl0_upper_addr, boot_lvl0_upper_size, boot_lvl0_upper),
            (boot_lvl1_upper_addr, boot_lvl1_upper_size, boot_lvl1_upper),
            (boot_lvl2_upper_addr, boot_lvl2_upper_size, boot_lvl2_upper),
            (boot_lvl2_lower_addr, boot_lvl2_lower_size, boot_lvl2_lower),
        ]
    }
}
