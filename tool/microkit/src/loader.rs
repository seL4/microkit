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
use std::mem;
use std::ops::Range;
use std::path::Path;

// allow(unused) because this includes "shared" types we don't want to construct
#[allow(unused)]
mod shared_types;

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

// Keep in sync with C's 'loader_region'
#[repr(C)]
struct LoaderRegion64 {
    load_addr: u64,
    size: u64,
    offset: u64,
    r#type: u64,
}

// Keep in sync with C's 'loader_header'
#[repr(C)]
struct LoaderHeader64 {
    magic: u64,
    size: u64,
    kernel_entry: u64,
    ui_p_reg_start: u64,
    ui_p_reg_end: u64,
    pv_offset: u64,
    v_entry: u64,
    kernel_first_vaddr: u64,
    kernel_first_paddr: u64,

    loader_regions_offset: u64,
    loader_regions_count: u64,

    mmu_regions_offset: u64,
    mmu_regions_count: u64,
}

pub struct Loader<'a> {
    arch: Arch,
    loader_image: Vec<u8>,
    header: LoaderHeader64,
    region_metadata: Vec<LoaderRegion64>,
    regions: Vec<(u64, &'a [u8])>,
    mmu_regions: Vec<shared_types::MmuRegion>,
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
        let loader_image = image_segment.data().clone();

        let kernel_first_vaddr = kernel_first_vaddr.expect("could find kernel vaddr");
        let kernel_first_paddr = kernel_first_paddr.expect("could find kernel paddr");

        if image_vaddr != loader_elf.entry {
            panic!("The loader entry point must be the first byte in the image");
        }

        assert_eq!(image_segment.mem_size(), image_segment.file_size());

        let kernel_entry = kernel_elf.entry;

        // initial task virt + pv_offset == initial task physical, so
        // pv_offset == initial task physical - initial task virt
        let pv_offset = initial_task_phy_base.wrapping_sub(initial_task_vaddr_range.start);

        let ui_p_reg_start = initial_task_phy_base;
        let ui_p_reg_end =
            ui_p_reg_start + (initial_task_vaddr_range.end - initial_task_vaddr_range.start);
        assert!(ui_p_reg_end > ui_p_reg_start);

        let mut region_metadata = Vec::new();
        // This offset is relative to the start of the loader *data* regions.
        let mut loader_data_offset: u64 = 0;
        for (addr, data) in &regions {
            region_metadata.push(LoaderRegion64 {
                load_addr: *addr,
                size: data.len() as u64,
                offset: loader_data_offset,
                r#type: 1,
            });
            loader_data_offset += data.len() as u64;
        }

        // let mmu_regions = vec![
        //     shared_types::MmuRegion {
        //         start: 0x60000000,
        //         top: 0xc0000000 - 1,
        //         arch_attrs: shared_types::MmuRegionArchAttrs { is_ram: true },
        //     },
        //     shared_types::MmuRegion {
        //         start: 0x9000000,
        //         top: 0x9000000 + 0xfff,
        //         arch_attrs: shared_types::MmuRegionArchAttrs { is_ram: false },
        //     },
        // ];

        let mmu_regions = vec![
            shared_types::MmuRegion {
                start: 0x40000000,
                top: 0x80000000 - 1,
                arch_attrs: shared_types::MmuRegionArchAttrs { is_ram: true },
            },
            shared_types::MmuRegion {
                start: 0x30860000,
                top: 0x30860000 + 0xfff,
                arch_attrs: shared_types::MmuRegionArchAttrs { is_ram: false },
            },
        ];

        // let mmu_regions = vec![
        //     shared_types::MmuRegion {
        //         start: 0x80200000,
        //         top: 0x100000000 - 1,
        //         arch_attrs: shared_types::MmuRegionArchAttrs { is_ram: true },
        //     }
        // ];

        let mmu_regions_offset = mem::size_of::<LoaderHeader64>() as u64;
        let loader_regions_offset = mmu_regions_offset
            + (mmu_regions.len() * mem::size_of::<shared_types::MmuRegion>()) as u64;
        let end_of_loader_offset = loader_regions_offset
            + (region_metadata.len() * mem::size_of::<LoaderRegion64>()) as u64
            + loader_data_offset;

        let size = loader_image.len() as u64 + end_of_loader_offset;

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
            kernel_first_vaddr,
            kernel_first_paddr,
            loader_regions_offset,
            loader_regions_count: region_metadata.len() as u64,
            mmu_regions_offset,
            mmu_regions_count: mmu_regions.len() as u64,
        };

        Loader {
            arch: config.arch,
            loader_image,
            header,
            region_metadata,
            regions,
            mmu_regions,
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

        // The ordering here must match the offsets.
        let current_offset = bytes.len() - self.loader_image.len();
        assert_eq!(current_offset as u64, self.header.mmu_regions_offset);

        for mmu_region in self.mmu_regions.iter() {
            let mmu_region_bytes = unsafe { struct_to_bytes(mmu_region) };
            bytes.extend_from_slice(mmu_region_bytes);
        }

        let current_offset = bytes.len() - self.loader_image.len();
        assert_eq!(current_offset as u64, self.header.loader_regions_offset);

        // For each region, we need to copy the region metadata as well
        for region in &self.region_metadata {
            let region_metadata_bytes = unsafe { struct_to_bytes(region) };
            bytes.extend_from_slice(region_metadata_bytes);
        }
        // Now we can copy all the region data
        for (_, data) in &self.regions {
            bytes.extend_from_slice(data);
        }

        assert_eq!(bytes.len() as u64, self.header.size);

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
            None,
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
}
