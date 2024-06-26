//
// Copyright 2024, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::fs;
use std::path::Path;
use std::collections::HashMap;
use crate::util::bytes_to_struct;

const STB_GLOBAL: u8 = 1;

#[repr(C, packed)]
struct ElfHeader32 {
    ident_magic: u32,
    ident_class: u8,
    ident_data: u8,
    ident_version: u8,
    ident_osabi: u8,
    ident_abiversion: u8,
    _padding: [u8; 7],
    type_: u16,
    machine: u16,
    version: u32,
    entry: u32,
    phoff: u32,
    shoff: u32,
    flags: u32,
    ehsize: u16,
    phentsize: u16,
    phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct ElfSymbol64 {
    name: u32,
    info: u8,
    other: u8,
    shndx: u16,
    value: u64,
    size: u64,
}

#[repr(C, packed)]
struct ElfSectionHeader64 {
    name: u32,
    type_: u32,
    flags: u64,
    addr: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    addralign: u64,
    entsize: u64,
}

#[repr(C, packed)]
struct ElfProgramHeader64 {
    type_: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    paddr: u64,
    filesz: u64,
    memsz: u64,
    align: u64,
}

#[repr(C, packed)]
struct ElfHeader64 {
    ident_magic: u32,
    ident_class: u8,
    ident_data: u8,
    ident_version: u8,
    ident_osabi: u8,
    ident_abiversion: u8,
    _padding: [u8; 7],
    type_: u16,
    machine: u16,
    version: u32,
    entry: u64,
    phoff: u64,
    shoff: u64,
    flags: u32,
    ehsize: u16,
    phentsize: u16,
    phnum: u16,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
}

const ELF_MAGIC: &[u8; 4] = b"\x7FELF";

pub struct ElfSegment {
    pub data: Vec<u8>,
    pub phys_addr: u64,
    pub virt_addr: u64,
    pub loadable: bool,
    attrs: u32,
}

impl ElfSegment {
    pub fn mem_size(&self) -> u64 {
        self.data.len() as u64
    }

    pub fn is_writable(&self) -> bool {
        (self.attrs & ElfSegmentAttributes::Write as u32) != 0
    }

    pub fn is_readable(&self) -> bool {
        (self.attrs & ElfSegmentAttributes::Read as u32) != 0
    }

    pub fn is_executable(&self) -> bool {
        (self.attrs & ElfSegmentAttributes::Execute as u32) != 0
    }
}

enum ElfSegmentAttributes {
    /// Corresponds to PF_X
    Execute = 0x1,
    /// Corresponds to PF_W
    Write = 0x2,
    /// Corresponds to PF_R
    Read = 0x4,
}

pub struct ElfFile {
    pub word_size: usize,
    pub entry: u64,
    pub segments: Vec<ElfSegment>,
    symbols: HashMap<String, ElfSymbol64>,
}

impl ElfFile {
    pub fn from_path(path: &Path) -> Result<ElfFile, String> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => return Err(format!("Failed to read ELF '{}': {}", path.display(), err))
        };

        let magic = &bytes[0..4];
        if magic != ELF_MAGIC {
            return Err(format!("ELF '{}': magic check failed", path.display()));
        }

        let word_size;
        let hdr_size;

        let class = &bytes[4..5][0];
        match class {
            1 => {
                hdr_size = std::mem::size_of::<ElfHeader32>();
                word_size = 32;
            },
            2 => {
                hdr_size = std::mem::size_of::<ElfHeader64>();
                word_size = 64;
            },
            _ => return Err(format!("ELF '{}': invalid class '{}'", path.display(), class))
        };

        // Now need to read the header into a struct
        let hdr_bytes = &bytes[..hdr_size];
        let hdr = unsafe { bytes_to_struct::<ElfHeader64>(hdr_bytes) };

        // We have checked this above but we should check again once we actually cast it to
        // a struct.
        assert!(hdr.ident_magic.to_le_bytes() == *magic);
        assert!(hdr.ident_class == *class);

        if hdr.ident_data != 1 {
            return Err(format!("ELF '{}': incorrect endianness, only little endian architectures are supported", path.display()));
        }

        let entry = hdr.entry;

        // Read all the segments
        let mut segments = Vec::with_capacity(hdr.phnum as usize);
        for i in 0..hdr.phnum {
            let phent_start = hdr.phoff + (i * hdr.phentsize) as u64;
            let phent_end = phent_start + (hdr.phentsize as u64);
            let phent_bytes = &bytes[phent_start as usize..phent_end as usize];

            let phent = unsafe { bytes_to_struct::<ElfProgramHeader64>(phent_bytes) };

            let segment_start = phent.offset as usize;
            let segment_end = phent.offset as usize + phent.filesz as usize;
            let mut segment_data = Vec::from(&bytes[segment_start..segment_end]);
            let num_zeroes = (phent.memsz - phent.filesz) as usize;
            segment_data.resize(segment_data.len() + num_zeroes, 0);

            let segment = ElfSegment {
                data: segment_data,
                phys_addr: phent.paddr,
                virt_addr: phent.vaddr,
                loadable: phent.type_ == 1,
                attrs: phent.flags
            };

            segments.push(segment)
        }

        // Read all the section headers
        let mut shents = Vec::with_capacity(hdr.shnum as usize);
        let mut symtab_shent: Option<&ElfSectionHeader64> = None;
        let mut shstrtab_shent: Option<&ElfSectionHeader64> = None;
        for i in 0..hdr.shnum {
            let shent_start = hdr.shoff + (i * hdr.shentsize) as u64;
            let shent_end = shent_start + hdr.shentsize as u64;
            let shent_bytes = &bytes[shent_start as usize..shent_end as usize];

            let shent = unsafe { bytes_to_struct::<ElfSectionHeader64>(shent_bytes) };
            match shent.type_ {
                2 => symtab_shent = Some(shent),
                3 => shstrtab_shent = Some(shent),
                _ => {}
            }
            shents.push(shent);
        }

        if shstrtab_shent.is_none() {
            return Err(format!("ELF '{}': unable to find string table section", path.display()));
        }

        assert!(symtab_shent.is_some());
        if symtab_shent.is_none() {
            return Err(format!("ELF '{}': unable to find symbol table section", path.display()));
        }

        // Reading the symbol table
        let symtab_start = symtab_shent.unwrap().offset as usize;
        let symtab_end = symtab_start + symtab_shent.unwrap().size as usize;
        let symtab = &bytes[symtab_start..symtab_end];

        let symtab_str_shent = shents[symtab_shent.unwrap().link as usize];
        let symtab_str_start = symtab_str_shent.offset as usize;
        let symtab_str_end = symtab_str_start + symtab_str_shent.size as usize;
        let symtab_str = &bytes[symtab_str_start..symtab_str_end];

        // Read all the symbols
        let mut symbols: HashMap<String, ElfSymbol64> = HashMap::new();
        let mut offset = 0;
        let symbol_size = std::mem::size_of::<ElfSymbol64>();
        while offset < symtab.len() {
            let sym_bytes = &symtab[offset..offset + symbol_size];
            let (sym_head, sym_body, sym_tail) = unsafe { sym_bytes.align_to::<ElfSymbol64>() };
            assert!(sym_head.is_empty());
            assert!(sym_body.len() == 1);
            assert!(sym_tail.is_empty());

            let sym = sym_body[0];

            let bind = sym.info >> 4;
            let name = Self::get_string(symtab_str, sym.name as usize)?;
            // Here we are doing something that could end up being fairly expensive, we are copying
            // the string for each symbol name. It should be possible to turn this into a reference
            // although it might be awkward in order to please the borrow checker.
            let insert = symbols.insert(name.to_string(), sym);
            // We only care about duplicate symbols if it is a global symbol
            if insert.is_some() && bind == STB_GLOBAL {
                return Err(format!("ELF '{}: multiple symbols with name '{}'", path.display(), name));
            }
            offset += symbol_size;
        }

        Ok(ElfFile { word_size, entry, segments, symbols })
    }

    pub fn find_symbol(&self, variable_name: &str) -> Result<(u64, u64), String> {
        if let Some(sym) = self.symbols.get(variable_name) {
            Ok((sym.value, sym.size))
        } else {
            Err(format!("No symbol named '{variable_name}' not found"))
        }
    }

    pub fn write_symbol(&mut self, variable_name: &str, data: &[u8]) -> Result<(), String> {
        let (vaddr, size) = self.find_symbol(variable_name)?;
        for seg in &mut self.segments {
            if vaddr >= seg.virt_addr && vaddr + size <= seg.virt_addr + seg.data.len() as u64 {
                let offset = (vaddr - seg.virt_addr) as usize;
                assert!(data.len() as u64 <= size);
                seg.data[offset..offset + data.len()].copy_from_slice(data);
                return Ok(());
            }
        }

        Err(format!("No symbol named {} found", variable_name))
    }

    pub fn get_data(&self, vaddr: u64, size: u64) -> Option<&[u8]> {
        for seg in &self.segments {
            if vaddr >= seg.virt_addr && vaddr + size <= seg.virt_addr + seg.data.len() as u64 {
                let offset = (vaddr - seg.virt_addr) as usize;
                return Some(&seg.data[offset..offset + size as usize]);
            }
        }

        None
    }

    fn get_string(strtab: &[u8], idx: usize) -> Result<&str, String> {
        match strtab[idx..].iter().position(|&b| b == 0) {
            Some(null_byte_pos) => {
                let end_idx = idx + null_byte_pos;
                match std::str::from_utf8(&strtab[idx..end_idx]) {
                    Ok(string) => Ok(string),
                    Err(err) => Err(format!("Failed to convert strtab bytes to UTF-8 string: {}", err))
                }
            },
            None => Err(format!("Could not find null byte in strtab from index {}", idx)),
        }
    }
}
