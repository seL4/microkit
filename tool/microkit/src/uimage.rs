//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

// Document referenced:
// U-Boot: include/image.h
// Linux: https://www.kernel.org/doc/html/latest/arch/riscv/boot-image-header.html

use crate::struct_to_bytes;
use crate::{crc32::crc32, sel4::Arch};
use std::fs::File;
use std::io::Write;

const UIMAGE_NAME: &str = "seL4 Microkit";

// OS-type code
const IH_OS_LINUX: u8 = 5;

// CPU-arch codes
const IH_ARCH_RISCV: u8 = 26;

// Image-type code
const IH_TYPE_KERNEL: u8 = 2;

// No compression
const IH_COMP_NONE: u8 = 0;

// Image name length max
const IH_NMLEN: usize = 32;

// Image magic
const IH_MAGIC: u32 = 0x27051956;

#[repr(C, packed)]
struct UbootLegacyImgHeader {
    ih_magic: u32,           // Image Header Magic Number
    ih_hcrc: u32,            // Image Header CRC Checksum
    ih_time: u32,            // Image Creation Timestamp
    ih_size: u32,            // Image Data Size
    ih_load: u32,            // Data Load Address
    ih_ep: u32,              // Entry Point Address
    ih_dcrc: u32,            // Image Data CRC Checksum
    ih_os: u8,               // Operating System
    ih_arch: u8,             // CPU architecture
    ih_type: u8,             // Image Type
    ih_comp: u8,             // Compression Type
    ih_name: [u8; IH_NMLEN], // Image Name
}

// Bits 0:15 minor, bits 16:31 major
// Linux defines this to be 0.2 currently
const LINUX_RISCV_HEADER_VERSION: u32 = 2;

const LINUX_RISCV_HEADER_MAGIC: u64 = 0x5643534952;
const LINUX_RISCV_HEADER_MAGIC2: u32 = 0x05435352;

#[repr(C, packed)]
struct LinuxRiscvImageHeader {
    code0: u32,
    code1: u32,
    text_offset: u64, // Image load offset, little endian
    image_size: u64,  // Effective Image size, little endian
    flags: u64,       // kernel flags, little endian
    version: u32,     // Version of this header
    res1: u32,
    res2: u64,
    magic: u64,
    magic2: u32,
    res3: u32,
}

pub fn uimage_serialise(
    arch: &Arch,
    entry: u32,
    executable_payload: Vec<u8>,
    path: &std::path::Path,
) -> Result<u64, String> {
    let ih_arch_le = match arch {
        Arch::Aarch64 => unreachable!("internal bug: unimplemented uImage creation for ARM"),
        Arch::Riscv64 => IH_ARCH_RISCV,
        Arch::X86_64 => unreachable!("internal bug: unimplemented uImage creation for x86"),
    };

    // We masquerade the Microkit loader as a Linux kernel to U-Boot, so that U-Boot follows
    // the Linux boot protocol and give us the HART ID in a0. If we pack the uImage as a generic `IH_OS_ELF`,
    // we won't get the HART ID which will be problematic for booting seL4.
    let linux_riscv_hdr = LinuxRiscvImageHeader {
        code0: 0,
        code1: 0,
        text_offset: 0,
        image_size: executable_payload.len() as u64,
        flags: 0, // little endian executable
        version: LINUX_RISCV_HEADER_VERSION,
        res1: 0,
        res2: 0,
        magic: LINUX_RISCV_HEADER_MAGIC,
        magic2: LINUX_RISCV_HEADER_MAGIC2,
        res3: 0,
    };

    let mut linux_image_payload = Vec::new();
    linux_image_payload.extend_from_slice(unsafe { struct_to_bytes(&linux_riscv_hdr) });
    linux_image_payload.extend_from_slice(executable_payload.as_slice());

    // The actual loader executable is after the Linux header, so we tell U-Boot to load the
    // uImage in a way that the loader always start at the physical address it expects.
    let load_paddr = entry - ::core::mem::size_of::<LinuxRiscvImageHeader>() as u32;

    let mut hdr = UbootLegacyImgHeader {
        ih_magic: IH_MAGIC.to_be(),
        ih_hcrc: 0, // U-Boot clears this field before it recalculate the checksum, so do the same here
        ih_time: 0,
        ih_size: (linux_image_payload.len() as u32).to_be(),
        ih_load: load_paddr.to_be(),
        ih_ep: entry.to_be(),
        ih_dcrc: crc32(&linux_image_payload).to_be(),
        ih_os: IH_OS_LINUX.to_be(),
        ih_arch: ih_arch_le.to_be(),
        ih_type: IH_TYPE_KERNEL.to_be(),
        ih_comp: IH_COMP_NONE.to_be(),
        ih_name: [0; IH_NMLEN],
    };

    let hdr_name_cpy_dest = &mut hdr.ih_name[0..UIMAGE_NAME.len()];
    hdr_name_cpy_dest.copy_from_slice(UIMAGE_NAME.as_bytes());

    let hdr_chksum = unsafe { crc32(struct_to_bytes(&hdr)) };
    hdr.ih_hcrc = hdr_chksum.to_be();

    let mut uimage_file = match File::create(path) {
        Ok(file) => file,
        Err(e) => return Err(format!("cannot create '{}': {}", path.display(), e)),
    };

    uimage_file
        .write_all(unsafe { struct_to_bytes(&hdr) })
        .unwrap_or_else(|_| panic!("Failed to write uImage header for '{}'", path.display()));

    uimage_file
        .write_all(&linux_image_payload)
        .unwrap_or_else(|_| panic!("Failed to write payload for '{}'", path.display()));

    uimage_file.flush().unwrap();

    Ok(0)
}
