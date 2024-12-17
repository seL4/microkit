//
// Copyright 2024, Neutrality Sàrl
//
// SPDX-License-Identifier: BSD-2-Clause
//

use crate::elf::{ElfFile,ElfSegment,ElfSegmentAttributes};
use crate::sel4::Config;
use crate::round_up;
use crate::MemoryRegion;
use std::path::Path;

pub struct X86Loader {
    elf: ElfFile,
}

impl X86Loader {
    pub fn new(
        _config: &Config,
        loader_elf_path: &Path,
        kernel_elf: &ElfFile,
        initial_task_elf: &ElfFile,
        _initial_task_phys_base: Option<u64>,
        reserved_region: MemoryRegion,
        system_regions: Vec<(u64, &[u8])>,
    ) -> X86Loader {

        // Load the loader ELF file.
        let mut elf = ElfFile::from_path(loader_elf_path).unwrap();

        // Add the PD memory regions as segments.
        for region in system_regions {
            let segment = ElfSegment {
                data: region.1.to_vec(),
                phys_addr: region.0,
                virt_addr: 0,
                loadable: true,
                attrs: ElfSegmentAttributes::Read as u32,
            };
            elf.add_segment(segment);
        }

        // Add the kernel memory regions as segments.
        for segment in &kernel_elf.segments {
            // Wipe the virtual address fields that are unnecessary and
            // cause issues since they are 64-bit wide.
            let mut segment = segment.clone();
            segment.virt_addr = 0;
            elf.add_segment(segment);
        }

        // Save the kernel's entry point address so we can jump into it
        // once we're done with our boot dance.
        let entry = kernel_elf.entry as u32;
        elf.write_symbol("kernel_entry", &entry.to_le_bytes()).unwrap();

        // Save the address and size of the reserved memory region that
        // holds the PD regions and the monitor invocation table.
        elf.write_symbol("extra_device_addr_p", &reserved_region.base.to_le_bytes()).unwrap();
        elf.write_symbol("extra_device_size", &reserved_region.size().to_le_bytes()).unwrap();

        // # Export the monitor task as a binary ELF64 file in memory.
        let mut monitor_raw = std::io::Cursor::new(Vec::new());
        initial_task_elf.write(&mut monitor_raw).unwrap();

        // Add the monitor ELF file as a segment to the loader, and
        // have it loaded at a page aligned address just after the
        // loader.
        let (bss_end, _) = elf.find_symbol("_bss_end").unwrap();
        let monitor_addr = round_up(bss_end as usize, 0x1000) as u32;
        let monitor_size = monitor_raw.get_ref().len() as u32;
        let monitor_segment = ElfSegment {
            data: monitor_raw.get_ref().to_vec(),
            phys_addr: monitor_addr as u64,
            virt_addr: 0,
            loadable: true,
            attrs: ElfSegmentAttributes::Read as u32,
        };
        elf.add_segment(monitor_segment);

        // Save the monitor's loaded address and size.
        elf.write_symbol("monitor_addr", &monitor_addr.to_le_bytes()).unwrap();
        elf.write_symbol("monitor_size", &monitor_size.to_le_bytes()).unwrap();

        X86Loader {
            elf: elf,
        }
    }

    pub fn write_image(&self, path: &Path) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        self.elf.write(&mut file)?;
        Ok(())
    }
}
