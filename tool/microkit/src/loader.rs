//
// Copyright 2024, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//
use crate::elf::{ElfFile, ElfSegmentData};
use crate::sel4::{Arch, Config, PlatformConfigRegion};
use crate::uimage::uimage_serialise;
use crate::util::{align_down, mb, round_up, struct_to_bytes};
use std::cmp::min;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::mem;
use std::ops::Range;
use std::path::Path;

macro_rules! grab_symbol {
    ($elf: expr, $symbol_name: expr) => {
        $elf.find_symbol($symbol_name)
            .expect(concat!("Could not find '", $symbol_name, "' symbol"))
    };
}

// XX: This could be generic on arbitrary <T> if we could specify T:: implements from_le_bytes,
// but we can't.
fn read_symbol_maybe(elf: &ElfFile, symbol_name: &str) -> Option<u64> {
    let (addr, size) = elf.find_symbol(symbol_name).ok()?;

    let symbol_bytes = elf.get_data(addr, size)?;

    assert!(mem::size_of::<u64>() == symbol_bytes.len());

    Some(u64::from_le_bytes(symbol_bytes.try_into().ok()?))
}

macro_rules! write_symbol {
    ($loader_image: expr, $image_vaddr: expr, $elf: expr, $symbol: literal, $symbol_var: expr) => {
        let (addr, size) = grab_symbol!($elf, $symbol);
        let addr = usize::try_from(addr).expect("addr fits in usize");
        let size = usize::try_from(size).expect("size fits in usize");
        let image_vaddr = usize::try_from($image_vaddr).expect("vaddr fits in usize");

        assert!(addr >= image_vaddr);
        assert!(size == ::std::mem::size_of_val(&$symbol_var));

        let offset: usize = (addr - image_vaddr);
        assert!(offset <= $loader_image.len());

        $loader_image[offset..(offset + size)].copy_from_slice(&$symbol_var.to_le_bytes());
    };
}

const PAGE_TABLE_SIZE: usize = 4096;

pub mod aarch64 {
    //! For AArch64, our page tables use the Stage 1 descriptor formats
    //! for both EL2 (TTBR0_EL2) and EL1 (TTBR0_EL1/TTBR1_EL1).
    //! Stage 2 descriptors are only used when in the EL1&0 regime; which is not
    //! the case when in EL2.

    use crate::util::mask;

    pub const LVL0_BITS: u64 = 9;
    pub const LVL1_BITS: u64 = 9;
    pub const LVL2_BITS: u64 = 9;
    pub const LVL3_BITS: u64 = 9;

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

    pub fn lvl3_index(addr: u64) -> usize {
        let idx = (addr >> PAGE_BITS_4KB) & mask(LVL3_BITS);
        idx as usize
    }

    /// Stage 1 translation table page/block descriptors have bits[4:2] containing
    /// AttrIndex[2:0]. The AttrIndex values depends on our configuration of
    /// the `MAIR_EL1` or `MAIR_EL2` registers done in util64.S;
    /// This also needs to match the values that seL4 uses.
    #[allow(non_upper_case_globals, reason = "matching ARM naming convention")]
    pub mod s1_mair_attr_index {
        pub const MT_DEVICE_nGnRnE: u64 = 0b000;
        pub const MT_DEVICE_nGnRE: u64 = 0b001;
        pub const MT_DEVICE_GRE: u64 = 0b010;
        pub const MT_NORMAL_NC: u64 = 0b011;
        pub const MT_NORMAL: u64 = 0b100;
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
        //! memory.

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

    // TODO:

    pub const BLOCK_BITS_512GB: u64 = 39;
    pub const PAGE_BITS_4KB: u64 = 12;

    /// Per "Table D8-52 Stage 1 VMSAv8-64 Block and Page descriptor fields" and
    /// "Figure D8-14 VMSAv8-64 Block descriptor formats" of ARM DDI0487L.b;
    /// specifically subfigure "4KB, 16KB, and 64KB granules, 48-bit OA"
    pub fn block_descriptor(level: usize, addr: u64, attr_index: u64) -> u64 {
        // Per Table D8-48, Condition for descriptor_type::BLOCK is level != 3.
        assert!(level != 3);

        let upper_attributes: u64 = 0;

        let shareability = if attr_index == s1_mair_attr_index::MT_NORMAL {
            // Match what the seL4 kernel uses for its page tables, which
            // is especially necessary for SMP booting which relies on it
            // for coherency. See the comment in seL4 `release_secondary_cpus()`.
            shareability_attributes::INNER_SHAREABLE
        } else {
            // Per $R_{PYFVQ}$:
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
        // bit[10] is the access flag; depending on FEAT_HAFDBS, when software
        //         manages the AF memory accesses to the page/block when AF=0
        //         raise an Access Fault; when hardware manages the AF it will
        //         become 1.
        // bit[9:8] is SH[1:0] containing stage 1 shareability attributes
        // bit[7:6] contains AP[2:1]
        // bit[5] is RES0
        // bit[4:2] contains AttrIndex
        let lower_attributes: u64 =
            (1 << 10) | (AP_KERNEL_RW << 6) | (shareability << 8) | (attr_index << 2);

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
    pub fn page_descriptor(addr: u64, attr_index: u64) -> u64 {
        // The main difference between a page descriptor and block descriptor
        // is in the size of the output address (OA) and in the descriptor type.

        let upper_attributes: u64 = 0;

        let shareability = if attr_index == s1_mair_attr_index::MT_NORMAL {
            // Match what the seL4 kernel uses for its page tables, which
            // is especially necessary for SMP booting which relies on it
            // for coherency.
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
        //   stage 1: 0b00 is {PrivRead, PrivWrite} and we are EL1/El2 (priv)
        const AP_KERNEL_RW: u64 = 0b00;

        // bit[11] is the not global (nG) field, we leave as 0 (global).
        // bit[10] is the access flag; depending on FEAT_HAFDBS, when software
        //         manages the AF memory accesses to the page/block when AF=0
        //         raise an Access Fault; when hardware manages the AF it will
        //         become 1.
        // bit[9:8] is SH[1:0] containing stage 1 shareability attributes
        // bit[7:6] contains AP[2:1]
        // bit[5] is RES0
        // bit[4:2] contains AttrIndex
        let lower_attributes: u64 =
            (1 << 10) | (AP_KERNEL_RW << 6) | (shareability << 8) | (attr_index << 2);

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
    pub(crate) const BLOCK_BITS_1GB: u64 = 30;
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
fn check_non_overlapping(regions: &Vec<(u64, u64)>) {
    let mut checked: Vec<(u64, u64)> = Vec::new();
    for &(base, size) in regions.iter() {
        let end = base + size;
        // Check that this does not overlap with any checked regions
        for &(b, e) in checked.iter() {
            if !(end <= b || base >= e) {
                panic!("Overlapping regions: [{base:x}..{end:x}) overlaps [{b:x}..{e:x})");
            }
        }

        checked.push((base, end));
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
    page_table_bytes: Vec<u8>,
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

        let image_segment = loader_elf
            .segments
            .iter()
            .find(|segment| segment.loadable)
            .expect("Did not find loadable segment");

        // Called "vaddr" but due to 1:1 mapping vaddr == paddr.
        let image_vaddr = image_segment.virt_addr;

        // We have to clone here as the image executable is part of this function return object,
        // and the loader ELF is deserialised in this scope, so its lifetime will be shorter than
        // the return object.
        let mut loader_image = image_segment.data().clone();

        if image_vaddr != loader_elf.entry {
            panic!("The loader entry point must be the first byte in the image");
        }

        let kernel_entry = kernel_elf.entry;

        // initial task virt + pv_offset == initial task physical, so
        // pv_offset == initial task physical - initial task virt
        let pv_offset = initial_task_phy_base.wrapping_sub(initial_task_vaddr_range.start);

        let ui_p_reg_start = initial_task_phy_base;
        let ui_p_reg_end =
            ui_p_reg_start + (initial_task_vaddr_range.end - initial_task_vaddr_range.start);
        assert!(ui_p_reg_end > ui_p_reg_start);

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

        let partial_size = loader_image.len() as u64
            + mem::size_of::<LoaderHeader64>() as u64
            + (region_metadata.len() * mem::size_of::<LoaderRegion64>()) as u64
            + offset;

        let page_tables_paddr_start = image_vaddr + partial_size;

        let mut page_table_bytes = Vec::<u8>::new();
        match config.arch {
            Arch::Aarch64 => {
                let (ttbr0_el2, ttbr0_el1, ttbr1_el1) = Loader::aarch64_setup_pagetables(
                    config,
                    &loader_elf,
                    kernel_first_vaddr,
                    kernel_first_paddr,
                    page_tables_paddr_start,
                    &mut page_table_bytes,
                );

                write_symbol!(
                    loader_image,
                    image_vaddr,
                    loader_elf,
                    "aarch64_pt_ttbr0_el2",
                    ttbr0_el2
                );
                write_symbol!(
                    loader_image,
                    image_vaddr,
                    loader_elf,
                    "aarch64_pt_ttbr0_el1",
                    ttbr0_el1
                );
                write_symbol!(
                    loader_image,
                    image_vaddr,
                    loader_elf,
                    "aarch64_pt_ttbr1_el1",
                    ttbr1_el1
                );
            }
            Arch::Riscv64 => {
                let boot_lvl1_pt = Loader::riscv64_setup_pagetables(
                    config,
                    &loader_elf,
                    kernel_first_vaddr,
                    kernel_first_paddr,
                    page_tables_paddr_start,
                    &mut page_table_bytes,
                );
                write_symbol!(
                    loader_image,
                    image_vaddr,
                    loader_elf,
                    "riscv64_boot_lvl1_pt",
                    boot_lvl1_pt
                );
            }
            Arch::X86_64 => unreachable!("x86_64 does not support creating a loader image"),
        };

        let size = partial_size + page_table_bytes.len() as u64;

        let mut all_regions_with_loader: Vec<_> = regions
            .iter()
            .map(|&(base, data)| (base, data.len() as u64))
            .collect();
        all_regions_with_loader.push((image_vaddr, size));
        check_non_overlapping(&all_regions_with_loader);

        // TODO: Check contained within real RAM.

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
            page_table_bytes,
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

        bytes.extend_from_slice(&self.page_table_bytes);

        assert!(bytes.len() as u64 == self.header.size);

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

    /// RISC-V 64 page tables for our purposes uses the Sv39 translation scheme
    /// (3-level page tables).
    ///
    /// It is split into two halves: the Upper/Kernel part of the page tables,
    /// which matches the format seL4 expects. The lower half contains an
    /// identity mapped region for the loader.
    ///
    /// ```txt
    ///            (512 GiB)
    ///   512 +---- Level 1 ---+ 2^39
    ///       |                |
    ///       |     (empty)    |
    ///       |                |
    ///   k+1 +----------------+                   (1 GiB)
    ///       | Level 2 Kernel | ----------> +---- Level 2 ---+             +-------------+
    ///     k +----------------+             |                | ----------> | 2 MiB block |
    ///       |                |         511 |----------------|             +-------------+
    ///       |                |             |                | ----------> | 2 MiB block |
    ///       |                |         510 |----------------|             +-------------+
    ///       |                |             |                | ----------> | 2 MiB block |
    ///       |                |             |----------------|             +-------------
    ///       |                |                   (...)           (...)         (...)          Kernel Regions
    ///       |                |             |----------------|             +-------------+
    ///       |                |             |                | ----------> | 2 MiB block |
    ///       |                |         l+1 |----------------|             +-------------+
    ///       |                |             | Level 3 Kernel | ----+
    ///       |                |           l |----------------|     |
    ///       |                |             |                |     |           (2 MiB)
    ///       |                |             |                |     +-----> +-- Level 3 --+             +------------+
    ///       |                |             |                |             |             | ----------> | 4 KiB page |
    ///       |                |             |                |         511 |-------------|             +------------+
    ///       |                |             |     (empty)    |             |             | ----------> | 4 KiB page |
    ///       |     (empty)    |             |                |             |-------------|             +------------+
    ///       |                |             |                |             |             | ----------> | 4 KiB page |
    ///       |                |             |                |           m |-------------|             +------------+ p
    ///       |                |             |                |             |   (empty)   |
    ///       |                |             |                |             +-------------+
    ///       |                |             |                |
    ///       |                |           0 +----------------+
    ///       |                |
    ///       |                |
    ///       |                |
    ///       |                |
    ///       |                |
    ///   s+1 +----------------+                  (1 GiB)
    ///       | Level 2 Loader | ---------->  +-- Level 2 --+             +-------------+
    ///     s +----------------+              |             | ----------> | 2 MiB block |
    ///       |                |          511 +-------------+             +-------------+
    ///       |                |              |             | ----------> | 2 MiB block |
    ///       |    (empty)     |          510 +-------------+             +-------------+
    ///       |                |              |             | ----------> | 2 MiB block |
    ///       |                |              |-------------|             +-------------+
    ///     0 +----------------+              |             | ----------> | 2 MiB block |
    ///                                       |-------------|             +-------------+
    ///                                            (...)         (...)         (...)          Loader Regions
    ///                                       |-------------|             +-------------+
    ///                                       |             | ----------> | 2 MiB block |
    ///                                       |-------------|             +-------------+
    ///                                       |             | ----------> | 2 MiB block |
    ///                                     t +-------------+             +-------------+
    ///                                       |             |
    ///                                       |   (empty)   |
    ///                                       |             |
    ///                                       +-------------+
    ///
    ///
    /// Where:
    ///      k = align_down(kernel_first_vaddr, 1GiB),
    ///      l = align_down(kernel_first_vaddr, 2MiB),
    ///      m = align_down(kernel_first_vaddr, 4KiB),
    ///      p = align_down(kernel_first_paddr, 4KiB),
    ///
    ///      s = align_down(text_addr, 1GiB),
    ///      t = align_down(text_addr, 2MiB),
    /// ```
    ///
    fn riscv64_setup_pagetables(
        config: &Config,
        elf: &ElfFile,
        kernel_first_vaddr: u64,
        kernel_first_paddr: u64,
        page_tables_paddr_start: u64,
        page_table_bytes: &mut Vec<u8>,
    ) -> u64 {
        use riscv64::{pt_index, pte_leaf, pte_next, BLOCK_BITS_1GB, BLOCK_BITS_2MB, PAGE_BITS_4K};

        let (text_addr, _) = grab_symbol!(elf, "_text");

        // We map the loader using 2MB pages, so make sure the base is actually aligned.
        assert!(text_addr.is_multiple_of(1 << BLOCK_BITS_2MB));

        const PAGE_TABLE_ENTRIES: usize = PAGE_TABLE_SIZE / mem::size_of::<u64>();

        let mut serialise_page_table_to_paddr = {
            let page_tables_paddr_start = {
                let aligned_pt_paddr_start =
                    page_tables_paddr_start.next_multiple_of(PAGE_TABLE_SIZE as u64);
                if aligned_pt_paddr_start != page_tables_paddr_start {
                    let alignment_diff =
                        (aligned_pt_paddr_start - page_tables_paddr_start) as usize;
                    page_table_bytes.resize(alignment_diff, 0);
                }

                aligned_pt_paddr_start
            };

            // This maintains the current end of the PT array.
            let mut next_pt_paddr = page_tables_paddr_start;

            move |page_table: &mut [u64; PAGE_TABLE_ENTRIES]| -> u64 {
                let pt_paddr = next_pt_paddr;
                page_table_bytes.extend(page_table.iter().flat_map(|pte| pte.to_le_bytes()));
                next_pt_paddr += PAGE_TABLE_SIZE as u64;
                page_table.fill(0);
                pt_paddr
            }
        };

        let num_pt_levels = config.riscv_pt_levels.unwrap().levels();
        assert!(num_pt_levels == 3);

        // Manufacture the constants as per the diagram.
        let k = align_down(kernel_first_vaddr, BLOCK_BITS_1GB);
        let l = align_down(kernel_first_vaddr, BLOCK_BITS_2MB);
        let m = align_down(kernel_first_vaddr, PAGE_BITS_4K);
        let p = align_down(kernel_first_paddr, PAGE_BITS_4K);

        let s = align_down(text_addr, BLOCK_BITS_1GB);
        let t = align_down(text_addr, BLOCK_BITS_2MB);

        // Manufacture the kernel page tables
        let kernel_lvl2_pt_paddr = {
            let mut lvl2_pt_kernel = [0u64; PAGE_TABLE_ENTRIES];

            let mut paddr = p;
            let index_l = pt_index(num_pt_levels, l, 2);

            lvl2_pt_kernel[index_l] = if kernel_first_vaddr.is_multiple_of(1 << BLOCK_BITS_2MB) {
                assert!(paddr.is_multiple_of(1 << BLOCK_BITS_2MB));
                let pte = pte_leaf(paddr);
                paddr += 1 << BLOCK_BITS_2MB;
                pte
            } else {
                let mut lvl3_pt_kernel = [0u64; PAGE_TABLE_ENTRIES];

                let index_m = pt_index(num_pt_levels, m, 3);

                for index in index_m..512 {
                    lvl3_pt_kernel[index] = pte_leaf(paddr);
                    paddr += 1 << PAGE_BITS_4K;
                }

                let kernel_lvl3_pt_paddr = serialise_page_table_to_paddr(&mut lvl3_pt_kernel);
                pte_next(kernel_lvl3_pt_paddr)
            };

            for index in (index_l + 1)..512 {
                lvl2_pt_kernel[index] = pte_leaf(paddr);
                paddr += 1 << BLOCK_BITS_2MB;
            }

            serialise_page_table_to_paddr(&mut lvl2_pt_kernel)
        };

        // Manufacture the loader page tables, which is relatively straightforward
        let loader_lvl2_pt_paddr = {
            let mut lvl2_pt_loader = [0u64; PAGE_TABLE_ENTRIES];

            // Identity mapped, so vaddr == paddr.
            let mut paddr = t;

            for index in pt_index(num_pt_levels, t, 2)..512 {
                lvl2_pt_loader[index] = pte_leaf(paddr);
                paddr += 1 << BLOCK_BITS_2MB;
            }

            serialise_page_table_to_paddr(&mut lvl2_pt_loader)
        };

        // Manufacture the Level 1 table
        let mut boot_lvl1_pt = [0u64; PAGE_TABLE_ENTRIES];

        let index_s = pt_index(num_pt_levels, s, 1);
        let index_k = pt_index(num_pt_levels, k, 1);
        boot_lvl1_pt[index_k] = pte_next(kernel_lvl2_pt_paddr);
        boot_lvl1_pt[index_s] = pte_next(loader_lvl2_pt_paddr);

        serialise_page_table_to_paddr(&mut boot_lvl1_pt)
    }

    /// AArch64 loader page tables have two variations:
    ///  - Loader in EL2, then Stage 1 translations in use, so we have the
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
    ///     0 +-------------+              TODO: RAM.
    ///
    ///
    /// Where:
    ///      k = align_down(kernel_first_vaddr, 512GiB),
    ///      l = align_down(kernel_first_vaddr, 1GiB),
    ///      m = align_down(kernel_first_vaddr, 2MiB),
    ///      p = align_down(kernel_first_paddr, 2MiB),
    ///      u = align_down(uart_base, 1GiB),
    /// ```
    ///
    fn aarch64_setup_pagetables(
        config: &Config,
        elf: &ElfFile,
        kernel_first_vaddr: u64,
        kernel_first_paddr: u64,
        page_tables_paddr_start: u64,
        page_table_bytes: &mut Vec<u8>,
    ) -> (u64, u64, u64) {
        use aarch64::{
            block_descriptor, lvl0_index, lvl1_index, lvl2_index, lvl3_index, page_descriptor,
            s1_mair_attr_index::{MT_DEVICE_nGnRnE, MT_NORMAL},
            table_descriptor, BLOCK_BITS_1GB, BLOCK_BITS_2MB, BLOCK_BITS_512GB, PAGE_BITS_4KB,
        };

        const PAGE_TABLE_ENTRIES: usize = PAGE_TABLE_SIZE / mem::size_of::<u64>();

        let mut serialise_page_table_to_paddr = {
            let page_tables_paddr_start = {
                let aligned_pt_paddr_start =
                    page_tables_paddr_start.next_multiple_of(PAGE_TABLE_SIZE as u64);
                if aligned_pt_paddr_start != page_tables_paddr_start {
                    let alignment_diff =
                        (aligned_pt_paddr_start - page_tables_paddr_start) as usize;
                    page_table_bytes.resize(alignment_diff, 0);
                }

                aligned_pt_paddr_start
            };

            // This maintains the current end of the PT array.
            let mut next_pt_paddr = page_tables_paddr_start;

            move |page_table: &mut [u64; PAGE_TABLE_ENTRIES]| -> u64 {
                let pt_paddr = next_pt_paddr;
                page_table_bytes.extend(page_table.iter().flat_map(|pte| pte.to_le_bytes()));
                next_pt_paddr += PAGE_TABLE_SIZE as u64;
                page_table.fill(0);
                pt_paddr
            }
        };

        let identity_mapped_regions = {
            let ram_regions = config
                .normal_regions
                .as_ref()
                .expect("AArch64 should have normal_regions");

            // println!("{:#x?}", ram_regions);

            let mut regions: Vec<_> = ram_regions
                .iter()
                .cloned()
                .map(|region| (region, MT_NORMAL))
                .collect();

            // FIXME: Derive from the kernel build system.
            if let Some(uart_base) = read_symbol_maybe(elf, "uart_addr") {
                let uart_base = align_down(uart_base, PAGE_BITS_4KB);
                regions.push((
                    PlatformConfigRegion {
                        start: uart_base,
                        end: uart_base + (1 << PAGE_BITS_4KB),
                    },
                    MT_DEVICE_nGnRnE,
                ));
            }

            // FIXME: This is currently assuming implementation details of the BCM2711/
            //        Raspberry Pi 4B spin table implementation, as it is the only
            //        platform we have that uses spin tables. Specifically, that
            //        it is always located at the 0 page.
            if elf.find_symbol("cpus_release_addr").is_ok() {
                regions.push((
                    PlatformConfigRegion {
                        start: 0x0,
                        end: 1 << PAGE_BITS_4KB,
                    },
                    MT_DEVICE_nGnRnE,
                ));
            }

            regions.sort_by_key(|(region, _)| region.start);

            regions
        };

        // Manufacture the constants as per the diagram.
        let k = align_down(kernel_first_vaddr, BLOCK_BITS_512GB);
        let l = align_down(kernel_first_vaddr, BLOCK_BITS_1GB);
        let m = align_down(kernel_first_vaddr, BLOCK_BITS_2MB);
        let p = align_down(kernel_first_paddr, BLOCK_BITS_2MB);

        // Manufacture the kernel page tables, which is relatively straightforward.
        let kernel_lvl1_pt_paddr = {
            // First, the Level 2 Upr table.
            let lvl2_pt_paddr = {
                let mut lvl2_pt_kernel = [0u64; PAGE_TABLE_ENTRIES];

                let mut vaddr = m;
                let mut paddr = p;
                while lvl1_index(m) == lvl1_index(vaddr) {
                    lvl2_pt_kernel[lvl2_index(vaddr)] = block_descriptor(2, paddr, MT_NORMAL);

                    vaddr += 1 << BLOCK_BITS_2MB;
                    paddr += 1 << BLOCK_BITS_2MB;
                }

                serialise_page_table_to_paddr(&mut lvl2_pt_kernel)
            };

            // Then, the Level 1 Upr table.
            let mut lvl1_pt_kernel = [0u64; PAGE_TABLE_ENTRIES];
            lvl1_pt_kernel[lvl1_index(l)] = table_descriptor(lvl2_pt_paddr);

            serialise_page_table_to_paddr(&mut lvl1_pt_kernel)
        };

        // Manufacture the RAM page tables, which is a little bit more complicated.
        // We assume that normal RAM lies between 0 <= paddr < 512GiB, i.e.
        // that lvl0_index(any ram region addr) = 0.
        let ram_lvl1_pt_paddr = {
            // Validation of assumptions about the identity mapped regions.
            let mut previous_end = None;
            for (region, _) in identity_mapped_regions.iter() {
                assert!(lvl0_index(region.start) == 0);
                assert!(lvl0_index(region.end - 1) == 0);
                // This is probably an unnecessary assumption.
                assert!(region.start.is_multiple_of(4096));
                assert!(region.end.is_multiple_of(4096));
                // This is definitely necessary.
                assert!(region.start >= previous_end.unwrap_or(0));
                previous_end = Some(region.end);
            }

            // We maintain three active page tables, which contain our previous
            // known page table data. As we process regions in ascending order,
            // once we have exceeded the bounds of the current reservation we
            // can simply push to the page_table_bytes storage and insert into
            // the parent PT the descriptor.
            // When the current vaddr (/paddr, as identity mapped) exceeds the
            // top value we rotate to a new PT.

            let mut lvl1_pt = [0u64; PAGE_TABLE_ENTRIES];
            let mut lvl2_pt = [0u64; PAGE_TABLE_ENTRIES];
            let mut lvl3_pt = [0u64; PAGE_TABLE_ENTRIES];
            // TODO: These should be defines. Note that the top is the size of 1 level of the next level up.
            // TODO: LVL1_ENTRY_RANGE? idk
            #[allow(unused_mut)]
            let mut lvl1_vaddr_top = 1 << BLOCK_BITS_512GB;
            let mut lvl2_vaddr_top = 1 << BLOCK_BITS_1GB;
            let mut lvl3_vaddr_top = 1 << BLOCK_BITS_2MB;

            // TODO: Tests...
            // This is similar to aligned_power_of_two_regions() for the kernel UT,
            // but we restrict it such that the output always is either 1GB, 2MB, or 4KB
            // pages.

            // Allowed externally for the final iteration
            let mut base = 0u64;
            for &(ref region, attr_index) in identity_mapped_regions.iter() {
                // println!("RAM Region: {:#x}..{:#x}", base, region.end);
                // println!(
                //     "  - Current Lvl1: {:#x}..{:#x}, entries: {}",
                //     (lvl1_vaddr_top - (1 << BLOCK_BITS_512GB)),
                //     lvl1_vaddr_top,
                //     lvl1_pt.iter().filter(|&&v| v != 0).count()
                // );
                // println!(
                //     "  - Current Lvl2: {:#x}..{:#x}, entries: {}",
                //     (lvl2_vaddr_top - (1 << BLOCK_BITS_1GB)),
                //     lvl2_vaddr_top,
                //     lvl2_pt.iter().filter(|&&v| v != 0).count()
                // );
                // println!(
                //     "  - Current Lvl3: {:#x}..{:#x}, entries: {}",
                //     (lvl3_vaddr_top - (1 << BLOCK_BITS_2MB)),
                //     lvl3_vaddr_top,
                //     lvl3_pt.iter().filter(|&&v| v != 0).count()
                // );

                // Handle the fact that the regions are not contiguous and that
                // we might need to skip PT.

                {
                    if region.start >= lvl3_vaddr_top {
                        if lvl3_pt != [0; _] {
                            let lvl3_pt_paddr = serialise_page_table_to_paddr(&mut lvl3_pt);
                            // println!("[iter] Serialise lvl3 table: {lvl3_pt_paddr:#x} for to {:#x}..{lvl3_vaddr_top:#x}", (lvl3_vaddr_top - (1 << BLOCK_BITS_2MB)));
                            assert!(lvl2_pt[lvl2_index(base)] == 0);
                            lvl2_pt[lvl2_index(base)] = table_descriptor(lvl3_pt_paddr);
                        }

                        // TODO: just compute it.
                        while region.start >= lvl3_vaddr_top {
                            lvl3_vaddr_top += 1 << BLOCK_BITS_2MB;
                        }
                    }

                    if region.start >= lvl2_vaddr_top {
                        if lvl2_pt != [0; _] {
                            let lvl2_pt_paddr = serialise_page_table_to_paddr(&mut lvl2_pt);
                            // println!("[iter] Serialise lvl2 table: {lvl2_pt_paddr:#x} for to {:#x}..{lvl2_vaddr_top:#x}, base: {:#x} lvl1_index(base): {:#x}", (lvl2_vaddr_top - (1 << BLOCK_BITS_1GB)), base, lvl1_index(base));
                            assert!(lvl1_pt[lvl1_index(base)] == 0);
                            lvl1_pt[lvl1_index(base)] = table_descriptor(lvl2_pt_paddr);
                        }

                        // TODO: just compute it.
                        while region.start >= lvl2_vaddr_top {
                            lvl2_vaddr_top += 1 << BLOCK_BITS_1GB;
                        }
                    }

                    if region.start >= lvl1_vaddr_top {
                        unreachable!(
                            "impossible as everything should fit here: {lvl1_vaddr_top:#x}"
                        );
                    }
                }

                // After serialising the old base, update the new one.
                base = region.start;

                // Inner Loop:
                // Invariant: the page tables in lvl1_pt, lvl2_pt, lvl3_pt
                //            are either (1) for the current address range,
                //            or (2) are empty and for a lower level than the current level.
                //            Also, the values in lvlXXX_vaddr_top are always correct (even if empty)
                //            Also contiguous within the loop.
                // Loop entry: (1) holds by work at the start of each region
                while base != region.end {
                    // Condition is !=, but assert that we never skip it.
                    assert!(base < region.end);

                    let size_bits = region.end.wrapping_sub(base).ilog2();
                    let align_bits = min(
                        size_bits,
                        // FIXME: Once MSRV is > 1.97, use .lowest_one() method.
                        if base == 0 {
                            size_bits
                        } else {
                            base.trailing_zeros()
                        },
                    );

                    // Match the size and alignment of the current region to
                    // the valid PT region sizes.
                    let (level, bits) = match u64::from(align_bits) {
                        BLOCK_BITS_1GB.. => (1, BLOCK_BITS_1GB),
                        BLOCK_BITS_2MB.. => (2, BLOCK_BITS_2MB),
                        PAGE_BITS_4KB.. => (3, PAGE_BITS_4KB),
                        0.. => panic!("impossible; regions should be aligned to 4K at least"),
                    };

                    let pt_region_size = 1u64 << bits;
                    let top = base + pt_region_size;

                    // println!("- Aligned PT region: {:#x}..{:#x} (size_bits: {}, align_bits: {}, bits: {})", base, top, size_bits, align_bits, bits);
                    // println!(
                    //     "  - Current Lvl1: {:#x}..{:#x}, entries: {}",
                    //     (lvl1_vaddr_top - (1 << BLOCK_BITS_512GB)),
                    //     lvl1_vaddr_top,
                    //     lvl1_pt.iter().filter(|&&v| v != 0).count()
                    // );
                    // println!(
                    //     "  - Current Lvl2: {:#x}..{:#x}, entries: {}",
                    //     (lvl2_vaddr_top - (1 << BLOCK_BITS_1GB)),
                    //     lvl2_vaddr_top,
                    //     lvl2_pt.iter().filter(|&&v| v != 0).count()
                    // );
                    // println!(
                    //     "  - Current Lvl3: {:#x}..{:#x}, entries: {}",
                    //     (lvl3_vaddr_top - (1 << BLOCK_BITS_2MB)),
                    //     lvl3_vaddr_top,
                    //     lvl3_pt.iter().filter(|&&v| v != 0).count()
                    // );

                    match level {
                        1 => {
                            // If it belongs in Level 1 PT, then it must go in
                            // lvl1 pt. By the inavariant, base < lvl1_vaddr_top.
                            assert!(base < lvl1_vaddr_top);
                            // top is <= lvl1_vaddr_top (the case where it is the topmost entry)
                            assert!(top <= lvl1_vaddr_top);

                            assert!(lvl1_pt[lvl1_index(base)] == 0);
                            lvl1_pt[lvl1_index(base)] = block_descriptor(1, base, attr_index);

                            if top == lvl1_vaddr_top {
                                // Invariant maintenance: if the new top would be now equal
                                // the end of the page table's region top, we need a new
                                // page table object and add it to the list.

                                // This should be possible to handle - we just need to break out of this loop
                                todo!("handle the case where top of lvl1 is occupied - this would be near the top of 512GiB");
                            }

                            // Invariant: Lower levels are empty.
                            assert!(lvl2_pt == [0; _]);
                            assert!(lvl3_pt == [0; _]);
                            // Invariant maintenance: vaddr_top is right range for current PT.
                            // it's empty so we need to increment the top to be current top (1G aligned) + 2MIB (512 lvl3 entries)
                            lvl3_vaddr_top = top + (1 << BLOCK_BITS_2MB);
                            // it's empty so we need to increment the top to be current top (1G aligned) + 1G (512 lvl2 entries)
                            lvl2_vaddr_top = top + (1 << BLOCK_BITS_1GB);
                        }
                        2 => {
                            // If it is a 2MiB block, it must go in the Level 2 PT;
                            // by our invariants: base < lvl2_vaddr_top and top <= lvl2_vaddr_top
                            assert!(base < lvl2_vaddr_top);
                            assert!(top <= lvl2_vaddr_top);

                            assert!(lvl2_pt[lvl2_index(base)] == 0);
                            lvl2_pt[lvl2_index(base)] = block_descriptor(2, base, attr_index);

                            if top == lvl2_vaddr_top {
                                // Invariant maintenance: keep for current address range.
                                // As we're the top of the range, we can serialise the table.

                                let lvl2_pt_paddr = serialise_page_table_to_paddr(&mut lvl2_pt);
                                // println!("Serialise lvl2 table: {lvl2_pt_paddr:#x} up to {lvl2_vaddr_top:#x}");
                                lvl2_vaddr_top += 1 << BLOCK_BITS_1GB;

                                lvl1_pt[lvl1_index(base)] = table_descriptor(lvl2_pt_paddr);

                                if top == lvl1_vaddr_top {
                                    todo!("handle the case where top of lvl1 is occupied - this would be near the top of 512GiB");
                                }
                            }

                            // Invariant: Lower levels are empty.
                            assert!(lvl3_pt == [0; _]);
                            // Invariant maintenance: vaddr_top is right range for current PT.
                            // it's empty so we need to increment the top to be current top (2MIB aligned) + 2MIB (512 lvl3 entries)
                            lvl3_vaddr_top = top + (1 << BLOCK_BITS_2MB);
                        }
                        3 => {
                            // If it is a 4K page, it must go in the Level 3 PT;
                            // by our invariants: base < lvl3_vaddr_top and top <= lvl3_vaddr_top
                            assert!(base < lvl3_vaddr_top);
                            assert!(top <= lvl3_vaddr_top);

                            assert!(lvl3_pt[lvl3_index(base)] == 0);
                            lvl3_pt[lvl3_index(base)] = page_descriptor(base, attr_index);

                            if top == lvl3_vaddr_top {
                                // Invariant maintenance: keep for current address range.
                                // As we're the top of the range, we can serialise the table.

                                let lvl3_pt_paddr = serialise_page_table_to_paddr(&mut lvl3_pt);
                                // println!("Serialise lvl3 table: {lvl3_pt_paddr:#x} for to {:#x}..{lvl3_vaddr_top:#x}", (lvl3_vaddr_top - (1 << BLOCK_BITS_2MB)));
                                lvl3_vaddr_top += 1 << BLOCK_BITS_2MB;

                                assert!(lvl2_pt[lvl2_index(base)] == 0);
                                lvl2_pt[lvl2_index(base)] = table_descriptor(lvl3_pt_paddr);

                                if top == lvl2_vaddr_top {
                                    let lvl2_pt_paddr = serialise_page_table_to_paddr(&mut lvl2_pt);
                                    // println!("Serialise lvl2 table: {lvl2_pt_paddr:#x} for to {:#x}..{lvl2_vaddr_top:#x}", (lvl2_vaddr_top - (1 << BLOCK_BITS_1GB)));
                                    lvl2_vaddr_top += 1 << BLOCK_BITS_1GB;

                                    assert!(lvl1_pt[lvl1_index(base)] == 0);
                                    lvl1_pt[lvl1_index(base)] = table_descriptor(lvl2_pt_paddr);

                                    if top == lvl1_vaddr_top {
                                        todo!("handle the case where top of lvl1 is occupied - this would be near the top of 512GiB");
                                    }
                                }
                            }

                            // Invariant: lower levels empty is vacuuously true
                        }
                        _ => unreachable!("level is 1..=3"),
                    }

                    base = base + pt_region_size;
                }
            }

            // By the loop invariant, we know that anything before has been serialised.
            // However, as we are at the end of the loop now, we might have
            // page tables that have been partially filled out, and we need to
            // serialise these.

            if lvl3_pt != [0; _] {
                let lvl3_pt_paddr = serialise_page_table_to_paddr(&mut lvl3_pt);
                // println!("[end] Serialise lvl3 table: {lvl3_pt_paddr:#x}");
                assert!(lvl2_pt[lvl2_index(base)] == 0);
                lvl2_pt[lvl2_index(base)] = table_descriptor(lvl3_pt_paddr);
            }

            if lvl2_pt != [0; _] {
                let lvl2_pt_paddr = serialise_page_table_to_paddr(&mut lvl2_pt);
                // println!("[end] Serialise lvl2 table: {lvl2_pt_paddr:#x} for to {:#x}..{lvl2_vaddr_top:#x}, base: {:#x} lvl1_index(base): {:#x}", (lvl2_vaddr_top - (1 << BLOCK_BITS_1GB)), base, lvl1_index(base));
                assert!(lvl1_pt[lvl1_index(base)] == 0);
                lvl1_pt[lvl1_index(base)] = table_descriptor(lvl2_pt_paddr);
            }

            // the level1 pt should not be empty. lol.
            assert!(lvl1_pt != [0; _]);

            // println!("New lvl1 table");
            serialise_page_table_to_paddr(&mut lvl1_pt)
        };

        // Depending on whether we are in hypervisor mode, we either need to
        // return the TTBR0_EL2 or TTBR[0,1]_EL1 values. We return u64::MAX
        // so as to return garbage - an unaligned address outside of physical
        // memory.
        if config.hypervisor {
            // Manufacture the Level 0 table, containing the kernel table
            // and the RAM tables.

            let mut ttbr0_el2_pt = [0u64; PAGE_TABLE_ENTRIES];

            assert!(lvl0_index(k) != lvl0_index(0));
            ttbr0_el2_pt[lvl0_index(k)] = table_descriptor(kernel_lvl1_pt_paddr);
            ttbr0_el2_pt[lvl0_index(0)] = table_descriptor(ram_lvl1_pt_paddr);

            let ttbr0_el2 = serialise_page_table_to_paddr(&mut ttbr0_el2_pt);

            (ttbr0_el2, u64::MAX, u64::MAX)
        } else {
            let mut ttbr0_el1_pt = [0u64; PAGE_TABLE_ENTRIES];
            let mut ttbr1_el1_pt = [0u64; PAGE_TABLE_ENTRIES];

            // Kernel in TTBR1 (Upper)
            ttbr1_el1_pt[lvl0_index(k)] = table_descriptor(kernel_lvl1_pt_paddr);
            // Loader in TTBR0 (Lower)
            ttbr0_el1_pt[lvl0_index(0)] = table_descriptor(ram_lvl1_pt_paddr);

            let ttbr0_el1 = serialise_page_table_to_paddr(&mut ttbr0_el1_pt);
            let ttbr1_el1 = serialise_page_table_to_paddr(&mut ttbr1_el1_pt);

            (u64::MAX, ttbr0_el1, ttbr1_el1)
        }
    }
}
