//
// Copyright 2024, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use crate::elf::ElfFile;
use crate::sel4::{Arch, Config};
use crate::util::{kb, mask, mb, round_up, struct_to_bytes};
use crate::MemoryRegion;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

const PAGE_TABLE_SIZE: usize = 4096;

const AARCH64_1GB_BLOCK_BITS: u64 = 30;
const AARCH64_2MB_BLOCK_BITS: u64 = 21;

const AARCH64_LVL0_BITS: u64 = 9;
const AARCH64_LVL1_BITS: u64 = 9;
const AARCH64_LVL2_BITS: u64 = 9;

struct Aarch64;
impl Aarch64 {
    pub fn lvl0_index(addr: u64) -> usize {
        let idx = (addr >> (AARCH64_2MB_BLOCK_BITS + AARCH64_LVL2_BITS + AARCH64_LVL1_BITS))
            & mask(AARCH64_LVL0_BITS);
        idx as usize
    }

    pub fn lvl1_index(addr: u64) -> usize {
        let idx = (addr >> (AARCH64_2MB_BLOCK_BITS + AARCH64_LVL2_BITS)) & mask(AARCH64_LVL1_BITS);
        idx as usize
    }

    pub fn lvl2_index(addr: u64) -> usize {
        let idx = (addr >> (AARCH64_2MB_BLOCK_BITS)) & mask(AARCH64_LVL2_BITS);
        idx as usize
    }
}

struct Riscv64;
impl Riscv64 {
    const BLOCK_BITS_2MB: u64 = 21;

    const PAGE_TABLE_INDEX_BITS: u64 = 9;
    const PAGE_SHIFT: u64 = 12;
    /// This sets the page table entry bits: D,A,X,W,R.
    const PTE_TYPE_BITS: u64 = 0b11001110;
    // TODO: where does this come from?
    const PTE_TYPE_TABLE: u64 = 0;
    const PTE_TYPE_VALID: u64 = 1;

    const PTE_PPN0_SHIFT: u64 = 10;

    /// Due to RISC-V having various virtual memory setups, we have this generic function to
    /// figure out the page-table index given the total number of page table levels for the
    /// platform and which level we are currently looking at.
    pub fn pt_index(pt_levels: usize, addr: u64, level: usize) -> usize {
        let pt_index_bits = Self::PAGE_TABLE_INDEX_BITS * (pt_levels - level) as u64;
        let idx = (addr >> (pt_index_bits + Self::PAGE_SHIFT)) % 512;

        idx as usize
    }

    /// Generate physical page number given an address
    pub fn pte_ppn(addr: u64) -> u64 {
        (addr >> Self::PAGE_SHIFT) << Self::PTE_PPN0_SHIFT
    }

    pub fn pte_next(addr: u64) -> u64 {
        Self::pte_ppn(addr) | Self::PTE_TYPE_TABLE | Self::PTE_TYPE_VALID
    }

    pub fn pte_leaf(addr: u64) -> u64 {
        Self::pte_ppn(addr) | Self::PTE_TYPE_BITS | Self::PTE_TYPE_VALID
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
    flags: u64,
    kernel_entry: u64,
    ui_p_reg_start: u64,
    ui_p_reg_end: u64,
    pv_offset: u64,
    v_entry: u64,
    extra_device_addr_p: u64,
    extra_device_size: u64,
    num_regions: u64,
}

pub struct Loader<'a> {
    image: Vec<u8>,
    header: LoaderHeader64,
    region_metadata: Vec<LoaderRegion64>,
    regions: Vec<(u64, &'a [u8])>,
}

impl<'a> Loader<'a> {
    pub fn new(
        config: &Config,
        loader_elf_path: &Path,
        kernel_elf: &'a ElfFile,
        initial_task_elf: &'a ElfFile,
        initial_task_phys_base: Option<u64>,
        reserved_region: MemoryRegion,
        system_regions: Vec<(u64, &'a [u8])>,
    ) -> Loader<'a> {
        // Note: If initial_task_phys_base is not None, then it just this address
        // as the base physical address of the initial task, rather than the address
        // that comes from the initial_task_elf file.
        let elf = ElfFile::from_path(loader_elf_path).unwrap();
        let sz = elf.word_size;
        let magic = match sz {
            32 => 0x5e14dead,
            64 => 0x5e14dead14de5ead,
            _ => panic!(
                "Internal error: unexpected ELF word size: {} from '{}'",
                sz,
                loader_elf_path.display()
            ),
        };

        let mut regions = Vec::new();

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

                if kernel_p_v_offset.is_none() {
                    kernel_p_v_offset = Some(segment.virt_addr - segment.phys_addr);
                } else if kernel_p_v_offset.unwrap() != segment.virt_addr - segment.phys_addr {
                    panic!("Kernel does not have a consistent physical to virtual offset");
                }

                regions.push((segment.phys_addr, segment.data.as_slice()));
            }
        }

        assert!(kernel_first_paddr.is_some());

        // Note: This could be extended to support multi-segment ELF files
        // (and indeed initial did support multi-segment ELF files). However
        // it adds significant complexity, and the calling functions enforce
        // only single-segment ELF files, so we keep things simple here.
        let initial_task_segments: Vec<_> = initial_task_elf
            .segments
            .iter()
            .filter(|s| s.loadable)
            .collect();
        assert!(initial_task_segments.len() == 1);
        let segment = &initial_task_segments[0];
        assert!(segment.loadable);

        let inittask_first_vaddr = segment.virt_addr;
        let inittask_last_vaddr = round_up(segment.virt_addr + segment.mem_size(), kb(4));

        let inittask_first_paddr = match initial_task_phys_base {
            Some(paddr) => paddr,
            None => segment.phys_addr,
        };
        let inittask_p_v_offset = inittask_first_vaddr - inittask_first_paddr;

        // Note: For now we include any zeroes. We could optimize in the future
        regions.push((inittask_first_paddr, &segment.data));

        // Determine the pagetable variables
        assert!(kernel_first_vaddr.is_some());
        assert!(kernel_first_vaddr.is_some());
        let pagetable_vars = match config.arch {
            Arch::Aarch64 => Loader::aarch64_setup_pagetables(
                &elf,
                kernel_first_vaddr.unwrap(),
                kernel_first_paddr.unwrap(),
            ),
            Arch::Riscv64 => Loader::riscv64_setup_pagetables(
                config,
                &elf,
                kernel_first_vaddr.unwrap(),
                kernel_first_paddr.unwrap(),
            ),
        };

        let image_segment = elf
            .segments
            .into_iter()
            .find(|segment| segment.loadable)
            .expect("Did not find loadable segment");
        let image_vaddr = image_segment.virt_addr;
        let mut image = image_segment.data;

        if image_vaddr != elf.entry {
            panic!("The loader entry point must be the first byte in the image");
        }

        for (var_addr, var_size, var_data) in pagetable_vars {
            let offset = var_addr - image_vaddr;
            assert!(var_size == var_data.len() as u64);
            assert!(offset > 0);
            assert!(offset <= image.len() as u64);
            image[offset as usize..(offset + var_size) as usize].copy_from_slice(&var_data);
        }

        let kernel_entry = kernel_elf.entry;

        let pv_offset = inittask_first_paddr.wrapping_sub(inittask_first_vaddr);

        let ui_p_reg_start = inittask_first_paddr;
        let ui_p_reg_end = inittask_last_vaddr - inittask_p_v_offset;
        assert!(ui_p_reg_end > ui_p_reg_start);

        let v_entry = initial_task_elf.entry;

        let extra_device_addr_p = reserved_region.base;
        let extra_device_size = reserved_region.size();

        let mut all_regions = Vec::with_capacity(regions.len() + system_regions.len());
        for region_set in [regions, system_regions] {
            for r in region_set {
                all_regions.push(r);
            }
        }

        let mut all_regions_with_loader = all_regions.clone();
        all_regions_with_loader.push((image_vaddr, &image));
        check_non_overlapping(&all_regions_with_loader);

        let flags = match config.hypervisor {
            true => 1,
            false => 0,
        };

        let mut region_metadata = Vec::new();
        let mut offset: u64 = 0;
        for (addr, data) in &all_regions {
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
            flags,
            kernel_entry,
            ui_p_reg_start,
            ui_p_reg_end,
            pv_offset,
            v_entry,
            extra_device_addr_p,
            extra_device_size,
            num_regions: all_regions.len() as u64,
        };

        Loader {
            image,
            header,
            region_metadata,
            regions: all_regions,
        }
    }

    pub fn write_image(&self, path: &Path) {
        let loader_file = match File::create(path) {
            Ok(file) => file,
            Err(e) => panic!("Could not create '{}': {}", path.display(), e),
        };

        let mut loader_buf = BufWriter::new(loader_file);

        // First write out all the image data
        loader_buf
            .write_all(self.image.as_slice())
            .expect("Failed to write image data to loader");

        // Then we write out the loader metadata (known as the 'header')
        let header_bytes = unsafe { struct_to_bytes(&self.header) };
        loader_buf
            .write_all(header_bytes)
            .expect("Failed to write header data to loader");
        // For each region, we need to write out the region metadata as well
        for region in &self.region_metadata {
            let region_metadata_bytes = unsafe { struct_to_bytes(region) };
            loader_buf
                .write_all(region_metadata_bytes)
                .expect("Failed to write region metadata to loader");
        }

        // Now we can write out all the region data
        for (_, data) in &self.regions {
            loader_buf
                .write_all(data)
                .expect("Failed to write region data to loader");
        }

        loader_buf.flush().unwrap();
    }

    fn riscv64_setup_pagetables(
        config: &Config,
        elf: &ElfFile,
        first_vaddr: u64,
        first_paddr: u64,
    ) -> Vec<(u64, u64, [u8; PAGE_TABLE_SIZE])> {
        let (text_addr, _) = elf
            .find_symbol("_text")
            .expect("Could not find 'text' symbol");
        let (boot_lvl1_pt_addr, boot_lvl1_pt_size) = elf
            .find_symbol("boot_lvl1_pt")
            .expect("Could not find 'boot_lvl1_pt' symbol");
        let (boot_lvl2_pt_addr, boot_lvl2_pt_size) = elf
            .find_symbol("boot_lvl2_pt")
            .expect("Could not find 'boot_lvl2_pt' symbol");
        let (boot_lvl2_pt_elf_addr, boot_lvl2_pt_elf_size) = elf
            .find_symbol("boot_lvl2_pt_elf")
            .expect("Could not find 'boot_lvl2_pt_elf' symbol");

        let num_pt_levels = config.riscv_pt_levels.unwrap().levels();

        let mut boot_lvl1_pt: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];
        {
            let text_index_lvl1 = Riscv64::pt_index(num_pt_levels, text_addr, 1);
            let pt_entry = Riscv64::pte_next(boot_lvl2_pt_elf_addr);
            let start = 8 * text_index_lvl1;
            let end = start + 8;
            boot_lvl1_pt[start..end].copy_from_slice(&pt_entry.to_le_bytes());
        }

        let mut boot_lvl2_pt_elf: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];
        {
            let text_index_lvl2 = Riscv64::pt_index(num_pt_levels, text_addr, 2);
            for (page, i) in (text_index_lvl2..512).enumerate() {
                let start = 8 * i;
                let end = start + 8;
                let addr = text_addr + ((page as u64) << Riscv64::BLOCK_BITS_2MB);
                let pt_entry = Riscv64::pte_leaf(addr);
                boot_lvl2_pt_elf[start..end].copy_from_slice(&pt_entry.to_le_bytes());
            }
        }

        {
            let index = Riscv64::pt_index(num_pt_levels, first_vaddr, 1);
            let start = 8 * index;
            let end = start + 8;
            boot_lvl1_pt[start..end]
                .copy_from_slice(&Riscv64::pte_next(boot_lvl2_pt_addr).to_le_bytes());
        }

        let mut boot_lvl2_pt: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];

        {
            let index = Riscv64::pt_index(num_pt_levels, first_vaddr, 2);
            for (page, i) in (index..512).enumerate() {
                let start = 8 * i;
                let end = start + 8;
                let addr = first_paddr + ((page as u64) << Riscv64::BLOCK_BITS_2MB);
                let pt_entry = Riscv64::pte_leaf(addr);
                boot_lvl2_pt[start..end].copy_from_slice(&pt_entry.to_le_bytes());
            }
        }

        vec![
            (boot_lvl1_pt_addr, boot_lvl1_pt_size, boot_lvl1_pt),
            (boot_lvl2_pt_addr, boot_lvl2_pt_size, boot_lvl2_pt),
            (
                boot_lvl2_pt_elf_addr,
                boot_lvl2_pt_elf_size,
                boot_lvl2_pt_elf,
            ),
        ]
    }

    fn aarch64_setup_pagetables(
        elf: &ElfFile,
        first_vaddr: u64,
        first_paddr: u64,
    ) -> Vec<(u64, u64, [u8; PAGE_TABLE_SIZE])> {
        let (boot_lvl1_lower_addr, boot_lvl1_lower_size) = elf
            .find_symbol("boot_lvl1_lower")
            .expect("Could not find 'boot_lvl1_lower' symbol");
        let (boot_lvl1_upper_addr, boot_lvl1_upper_size) = elf
            .find_symbol("boot_lvl1_upper")
            .expect("Could not find 'boot_lvl1_upper' symbol");
        let (boot_lvl2_upper_addr, boot_lvl2_upper_size) = elf
            .find_symbol("boot_lvl2_upper")
            .expect("Could not find 'boot_lvl2_upper' symbol");
        let (boot_lvl0_lower_addr, boot_lvl0_lower_size) = elf
            .find_symbol("boot_lvl0_lower")
            .expect("Could not find 'boot_lvl0_lower' symbol");
        let (boot_lvl0_upper_addr, boot_lvl0_upper_size) = elf
            .find_symbol("boot_lvl0_upper")
            .expect("Could not find 'boot_lvl0_upper' symbol");

        let mut boot_lvl0_lower: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];
        boot_lvl0_lower[..8].copy_from_slice(&(boot_lvl1_lower_addr | 3).to_le_bytes());

        let mut boot_lvl1_lower: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];
        for i in 0..512 {
            #[allow(clippy::identity_op)] // keep the (0 << 2) for clarity
            let pt_entry: u64 = ((i as u64) << AARCH64_1GB_BLOCK_BITS) |
                (1 << 10) | // access flag
                (0 << 2) | // strongly ordered memory
                (1); // 1G block
            let start = 8 * i;
            let end = 8 * (i + 1);
            boot_lvl1_lower[start..end].copy_from_slice(&pt_entry.to_le_bytes());
        }

        let boot_lvl0_upper: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];
        {
            let pt_entry = (boot_lvl1_upper_addr | 3).to_le_bytes();
            let idx = Aarch64::lvl0_index(first_vaddr);
            boot_lvl0_lower[8 * idx..8 * (idx + 1)].copy_from_slice(&pt_entry);
        }

        let mut boot_lvl1_upper: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];
        {
            let pt_entry = (boot_lvl2_upper_addr | 3).to_le_bytes();
            let idx = Aarch64::lvl1_index(first_vaddr);
            boot_lvl1_upper[8 * idx..8 * (idx + 1)].copy_from_slice(&pt_entry);
        }

        let mut boot_lvl2_upper: [u8; PAGE_TABLE_SIZE] = [0; PAGE_TABLE_SIZE];

        let lvl2_idx = Aarch64::lvl2_index(first_vaddr);
        for i in lvl2_idx..512 {
            let entry_idx = (i - Aarch64::lvl2_index(first_vaddr)) << AARCH64_2MB_BLOCK_BITS;
            let pt_entry: u64 = (entry_idx as u64 + first_paddr) |
                (1 << 10) | // Access flag
                (3 << 8) | // Make sure the shareability is the same as the kernel's
                (4 << 2) | // MT_NORMAL memory
                (1 << 0); // 2MB block
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
        ]
    }
}
