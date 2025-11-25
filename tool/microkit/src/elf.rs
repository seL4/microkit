//
// Copyright 2024, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use crate::sel4::PageSize;
use crate::util::{bytes_to_struct, round_down, struct_to_bytes};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::{self, metadata, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::slice::from_raw_parts;

#[repr(C, packed)]
#[derive(Copy, Clone)]
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
#[derive(Clone, Eq, PartialEq)]
struct ElfSymbol64 {
    name: u32,
    info: u8,
    other: u8,
    shndx: u16,
    value: u64,
    size: u64,
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
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
#[derive(Copy, Clone)]
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
#[derive(Copy, Clone)]
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

const PHENT_TYPE_LOADABLE: u32 = 1;

/// ELF program-header flags (`p_flags`)
const PF_X: u32 = 0x1;
const PF_W: u32 = 0x2;
const PF_R: u32 = 0x4;

/// ELF section-header type (`sh_type`)
const SHT_PROGBITS: u32 = 0x1;

/// ELF section-header flags (`sh_flags`)
const SHF_WRITE: u64 = 0x1;
const SHF_ALLOC: u64 = 0x2;
const SHF_EXECINSTR: u64 = 0x3;

#[derive(Eq, PartialEq, Clone)]
pub enum ElfSegmentData {
    RealData(Vec<u8>),
    UninitialisedData(u64),
}

#[derive(Eq, PartialEq, Clone)]
pub struct ElfSegment {
    pub data: ElfSegmentData,
    pub phys_addr: u64,
    pub virt_addr: u64,
    pub loadable: bool,
    attrs: u32,
}

impl ElfSegment {
    pub fn mem_size(&self) -> u64 {
        match &self.data {
            ElfSegmentData::RealData(bytes) => bytes.len() as u64,
            ElfSegmentData::UninitialisedData(size) => *size,
        }
    }

    pub fn file_size(&self) -> u64 {
        match &self.data {
            ElfSegmentData::RealData(bytes) => bytes.len() as u64,
            ElfSegmentData::UninitialisedData(_) => 0,
        }
    }

    pub fn data(&self) -> &Vec<u8> {
        match &self.data {
            ElfSegmentData::RealData(bytes) => bytes,
            ElfSegmentData::UninitialisedData(_) => {
                unreachable!("internal bug: data() called on an uninitialised ELF segment.")
            }
        }
    }

    pub fn data_mut(&mut self) -> &mut Vec<u8> {
        match &mut self.data {
            ElfSegmentData::RealData(bytes) => bytes,
            ElfSegmentData::UninitialisedData(_) => {
                unreachable!("internal bug: data_mut() called on an uninitialised ELF segment.")
            }
        }
    }

    pub fn is_uninitialised(&self) -> bool {
        match &self.data {
            ElfSegmentData::RealData(_) => false,
            ElfSegmentData::UninitialisedData(_) => true,
        }
    }

    pub fn is_writable(&self) -> bool {
        self.attrs & PF_W == PF_W
    }

    pub fn is_readable(&self) -> bool {
        self.attrs & PF_R == PF_R
    }

    pub fn is_executable(&self) -> bool {
        self.attrs & PF_X == PF_X
    }

    pub fn section_flags(&self) -> u64 {
        let mut flags = SHF_ALLOC;

        if self.is_writable() {
            flags |= SHF_WRITE;
        }
        if self.is_executable() {
            flags |= SHF_EXECINSTR;
        }

        flags
    }
}

#[derive(Eq, PartialEq, Clone)]
pub struct ElfFile {
    pub path: PathBuf,
    pub word_size: usize,
    pub entry: u64,
    pub machine: u16,
    pub segments: Vec<ElfSegment>,
    symbols: HashMap<String, (ElfSymbol64, bool)>,
}

impl ElfFile {
    pub fn new(path: PathBuf, word_size: usize, entry: u64, machine: u16) -> Self {
        ElfFile {
            path,
            word_size,
            entry,
            machine,
            segments: [].into(),
            symbols: HashMap::new(),
        }
    }

    pub fn from_path(path: &Path) -> Result<ElfFile, String> {
        Self::from_split_paths(path, None)
    }

    pub fn from_split_paths(
        path: &Path,
        path_for_symbols: Option<&Path>,
    ) -> Result<ElfFile, String> {
        let reader = ElfFileReader::from_path(path)?;
        let reader_for_symbols = match path_for_symbols {
            Some(path_for_symbols) => Cow::Owned(ElfFileReader::from_path(path_for_symbols)?),
            None => Cow::Borrowed(&reader),
        };
        let segments = reader.segments()?;
        let symbols = reader_for_symbols.symbols()?;
        Ok(ElfFile {
            path: path.to_owned(),
            word_size: reader.word_size,
            entry: reader.hdr.entry,
            machine: reader.hdr.machine,
            segments,
            symbols,
        })
    }
}

#[derive(Clone)]
struct ElfFileReader<'a> {
    path: &'a Path,
    bytes: Vec<u8>,
    word_size: usize,
    hdr: ElfHeader64,
}

impl<'a> ElfFileReader<'a> {
    fn from_path(path: &'a Path) -> Result<Self, String> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => return Err(format!("Failed to read ELF '{}': {}", path.display(), err)),
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
            }
            2 => {
                hdr_size = std::mem::size_of::<ElfHeader64>();
                word_size = 64;
            }
            _ => {
                return Err(format!(
                    "ELF '{}': invalid class '{}'",
                    path.display(),
                    class
                ))
            }
        };

        if word_size != 64 {
            return Err(format!(
                "ELF '{}': unsupported word size: '{}'",
                path.display(),
                word_size
            ));
        }

        // Now need to read the header into a struct
        let hdr_bytes = &bytes[..hdr_size];
        let hdr = *unsafe { bytes_to_struct::<ElfHeader64>(hdr_bytes) };

        // We have checked this above but we should check again once we actually cast it to
        // a struct.
        assert!(hdr.ident_magic.to_le_bytes() == *magic);
        assert!(hdr.ident_class == *class);

        if hdr.ident_data != 1 {
            return Err(format!(
                "ELF '{}': incorrect endianness, only little endian architectures are supported",
                path.display()
            ));
        }

        Ok(Self {
            path,
            bytes,
            word_size,
            hdr,
        })
    }

    fn segments(&self) -> Result<Vec<ElfSegment>, String> {
        let hdr = &self.hdr;

        // Read all the segments
        if hdr.phnum == 0 {
            return Err(format!(
                "ELF '{}': has no program headers",
                self.path.display()
            ));
        }

        let mut segments = Vec::with_capacity(hdr.phnum as usize);
        for i in 0..hdr.phnum {
            let phent_start = hdr.phoff + (i * hdr.phentsize) as u64;
            let phent_end = phent_start + (hdr.phentsize as u64);
            let phent_bytes = &self.bytes[phent_start as usize..phent_end as usize];

            let phent = unsafe { bytes_to_struct::<ElfProgramHeader64>(phent_bytes) };

            let segment_start = phent.offset as usize;
            let segment_end = phent.offset as usize + phent.filesz as usize;

            if phent.type_ != PHENT_TYPE_LOADABLE {
                continue;
            }

            let mut segment_data_bytes = vec![0; phent.memsz as usize];
            segment_data_bytes[..phent.filesz as usize]
                .copy_from_slice(&self.bytes[segment_start..segment_end]);

            let segment_data = ElfSegmentData::RealData(segment_data_bytes);

            let flags = phent.flags;
            let segment = ElfSegment {
                data: segment_data,
                phys_addr: phent.paddr,
                virt_addr: phent.vaddr,
                loadable: phent.type_ == PHENT_TYPE_LOADABLE,
                attrs: flags,
            };

            segments.push(segment)
        }

        Ok(segments)
    }

    fn symbols(&self) -> Result<HashMap<String, (ElfSymbol64, bool)>, String> {
        let hdr = &self.hdr;

        // Read all the section headers
        let mut shents = Vec::with_capacity(hdr.shnum as usize);
        let mut symtab_shent: Option<&ElfSectionHeader64> = None;
        let mut shstrtab_shent: Option<&ElfSectionHeader64> = None;
        for i in 0..hdr.shnum {
            let shent_start = hdr.shoff + (i as u64 * hdr.shentsize as u64);
            let shent_end = shent_start + hdr.shentsize as u64;
            let shent_bytes = &self.bytes[shent_start as usize..shent_end as usize];

            let shent = unsafe { bytes_to_struct::<ElfSectionHeader64>(shent_bytes) };
            match shent.type_ {
                2 => symtab_shent = Some(shent),
                3 => shstrtab_shent = Some(shent),
                _ => {}
            }
            shents.push(shent);
        }

        if shstrtab_shent.is_none() {
            return Err(format!(
                "ELF '{}': unable to find string table section",
                self.path.display()
            ));
        }

        assert!(symtab_shent.is_some());
        if symtab_shent.is_none() {
            return Err(format!(
                "ELF '{}': unable to find symbol table section",
                self.path.display()
            ));
        }

        // Reading the symbol table
        let symtab_start = symtab_shent.unwrap().offset as usize;
        let symtab_end = symtab_start + symtab_shent.unwrap().size as usize;
        let symtab = &self.bytes[symtab_start..symtab_end];

        let symtab_str_shent = shents[symtab_shent.unwrap().link as usize];
        let symtab_str_start = symtab_str_shent.offset as usize;
        let symtab_str_end = symtab_str_start + symtab_str_shent.size as usize;
        let symtab_str = &self.bytes[symtab_str_start..symtab_str_end];

        // Read all the symbols
        let mut symbols: HashMap<String, (ElfSymbol64, bool)> = HashMap::new();
        let mut offset = 0;
        let symbol_size = std::mem::size_of::<ElfSymbol64>();
        while offset < symtab.len() {
            let sym_bytes = &symtab[offset..offset + symbol_size];
            let (sym_head, sym_body, sym_tail) = unsafe { sym_bytes.align_to::<ElfSymbol64>() };
            assert!(sym_head.is_empty());
            assert!(sym_body.len() == 1);
            assert!(sym_tail.is_empty());

            let sym = &sym_body[0];

            let name = Self::get_string(symtab_str, sym.name as usize)?;
            // It is possible for a valid ELF to contain multiple global symbols with the same name.
            // Because we are making the hash map of symbols now, what we do is keep track of how many
            // times we encounter the symbol name. Only when we go to find a particular symbol, do
            // we complain that it occurs multiple times.
            if let Some(symbol) = symbols.get_mut(name) {
                symbol.1 = true;
            } else {
                // Here we are doing something that could end up being fairly expensive, we are copying
                // the string for each symbol name. It should be possible to turn this into a reference
                // although it might be awkward in order to please the borrow checker.
                let insert = symbols.insert(name.to_string(), (sym.clone(), false));
                assert!(insert.is_none());
            }
            offset += symbol_size;
        }

        Ok(symbols)
    }
}

impl ElfFile {
    pub fn find_symbol(&self, variable_name: &str) -> Result<(u64, u64), String> {
        if let Some((sym, duplicate)) = self.symbols.get(variable_name) {
            if *duplicate {
                Err(format!("multiple symbols with name '{variable_name}'"))
            } else {
                Ok((sym.value, sym.size))
            }
        } else {
            Err(format!("no symbol named '{variable_name}' found"))
        }
    }

    pub fn write_symbol(&mut self, variable_name: &str, data: &[u8]) -> Result<(), String> {
        let (vaddr, size) = self.find_symbol(variable_name)?;
        for seg in &mut self.segments {
            if vaddr >= seg.virt_addr && vaddr + size <= seg.virt_addr + seg.mem_size() {
                let offset = (vaddr - seg.virt_addr) as usize;
                assert!(data.len() as u64 <= size);
                seg.data_mut()[offset..offset + data.len()].copy_from_slice(data);
                return Ok(());
            }
        }

        Err(format!("No symbol named {variable_name} found"))
    }

    pub fn get_data(&self, vaddr: u64, size: u64) -> Option<&[u8]> {
        for seg in &self.segments {
            if vaddr >= seg.virt_addr && vaddr + size <= seg.virt_addr + seg.mem_size() {
                let offset = (vaddr - seg.virt_addr) as usize;
                return Some(&seg.data()[offset..offset + size as usize]);
            }
        }

        None
    }
}

impl<'a> ElfFileReader<'a> {
    fn get_string(strtab: &[u8], idx: usize) -> Result<&str, String> {
        match strtab[idx..].iter().position(|&b| b == 0) {
            Some(null_byte_pos) => {
                let end_idx = idx + null_byte_pos;
                match std::str::from_utf8(&strtab[idx..end_idx]) {
                    Ok(string) => Ok(string),
                    Err(err) => Err(format!(
                        "Failed to convert strtab bytes to UTF-8 string: {err}"
                    )),
                }
            }
            None => Err(format!(
                "Could not find null byte in strtab from index {idx}"
            )),
        }
    }
}

impl ElfFile {
    pub fn lowest_vaddr(&self) -> u64 {
        // This unwrap is safe as we have ensured that there will always be at least 1 segment when parsing the ELF.
        let existing_vaddrs: Vec<u64> = self
            .loadable_segments()
            .iter()
            .map(|segm| segm.virt_addr)
            .collect();
        *existing_vaddrs.iter().min().unwrap()
    }

    pub fn highest_vaddr(&self) -> u64 {
        // This unwrap is safe as we have ensured that there will always be at least 1 segment when parsing the ELF.
        let existing_vaddrs: Vec<u64> = self
            .loadable_segments()
            .iter()
            .map(|segm| segm.virt_addr + segm.mem_size())
            .collect();
        *existing_vaddrs.iter().max().unwrap()
    }

    /// Returns the next available page aligned virtual address for inserting a new segment.
    pub fn next_vaddr(&self, page_size: PageSize) -> u64 {
        round_down(self.highest_vaddr() + page_size as u64, page_size as u64)
    }

    pub fn add_segment(
        &mut self,
        read: bool,
        write: bool,
        execute: bool,
        vaddr: u64,
        data: ElfSegmentData,
    ) {
        let r = if read { PF_R } else { 0 };
        let w = if write { PF_W } else { 0 };
        let x = if execute { PF_X } else { 0 };

        let elf_segment = ElfSegment {
            data,
            phys_addr: vaddr,
            virt_addr: vaddr,
            loadable: true,
            attrs: r | w | x,
        };
        self.segments.push(elf_segment);
    }

    pub fn loadable_segments(&self) -> Vec<&ElfSegment> {
        self.segments.iter().filter(|s| s.loadable).collect()
    }

    /// Re-create a minimal ELF file with all the segments.
    pub fn reserialise(&self, out: &std::path::Path) -> Result<u64, String> {
        let ehsize = size_of::<ElfHeader64>();

        let phnum = self.loadable_segments().len();
        let phentsize = size_of::<ElfProgramHeader64>();

        let shnum = self.loadable_segments().len() + 1; // First entry is reserved
        let shentsize = size_of::<ElfSectionHeader64>();

        let mut elf_file = match File::create(out) {
            Ok(file) => file,
            Err(e) => {
                return Err(format!(
                    "ELF: cannot reserialise '{}' to '{}': {}",
                    self.path.display(),
                    out.display(),
                    e
                ))
            }
        };

        // ELF header
        let header = ElfHeader64 {
            ident_magic: u32::from_le_bytes(*ELF_MAGIC),
            ident_class: 2, // 64-bits object
            ident_data: 1,  // little endian
            ident_version: 1,
            ident_osabi: 0,
            ident_abiversion: 0,
            _padding: [0; 7],
            type_: 2, // executable file
            machine: self.machine,
            version: 1,
            entry: self.entry,
            // Program headers starts after main header
            phoff: ehsize as u64,
            // Section headers starts after main header and program headers
            shoff: (ehsize + phnum * phentsize) as u64,
            flags: 0,
            ehsize: ehsize as u16,
            phentsize: phentsize as u16,
            phnum: phnum as u16,
            shentsize: shentsize as u16,
            shnum: shnum as u16,
            shstrndx: 0,
        };
        elf_file
            .write_all(unsafe {
                from_raw_parts((&header as *const ElfHeader64) as *const u8, ehsize)
            })
            .unwrap_or_else(|_| panic!("Failed to write ELF header for '{}'", out.display()));

        // Keep a running file offset where segment data will be written, which is after
        // all the various headers.
        let mut data_off_watermark = (ehsize as u64)
            + (phnum as u64) * (phentsize as u64)
            + (shnum as u64) * (shentsize as u64);
        let mut data_offs = [].to_vec();

        // First write out the program headers table
        for (i, seg) in self.loadable_segments().iter().enumerate() {
            let ph_serialised = ElfProgramHeader64 {
                type_: PHENT_TYPE_LOADABLE, // loadable
                flags: seg.attrs,
                offset: data_off_watermark,
                vaddr: seg.virt_addr,
                paddr: seg.phys_addr,
                filesz: seg.file_size(),
                memsz: seg.mem_size(),
                align: 0,
            };
            data_offs.push(data_off_watermark);

            elf_file
                .write_all(unsafe { struct_to_bytes(&ph_serialised) })
                .unwrap_or_else(|_| {
                    panic!(
                        "Failed to write ELF program header #{} for '{}'",
                        i,
                        out.display()
                    )
                });

            data_off_watermark += seg.file_size();
        }

        // Then the section headers table, which describe the same thing as the program headers.
        // This is needed for U-Boot's `bootelf` command to work properly without adding the `-p` flag
        // when booting the loader image on ARM and RISC-V platforms.
        // First entry is reserved!
        elf_file
            .write_all(vec![0u8; shentsize].as_slice())
            .unwrap_or_else(|_| {
                panic!(
                    "Failed to write ELF reserved segment header for '{}'",
                    out.display()
                )
            });
        for (i, seg) in self.loadable_segments().iter().enumerate() {
            let sh_serialised = ElfSectionHeader64 {
                name: 0,
                type_: SHT_PROGBITS,
                flags: seg.section_flags(),
                addr: seg.phys_addr,
                offset: data_offs[i],
                size: seg.file_size(),
                link: 0,
                info: 0,
                addralign: 0,
                entsize: 0,
            };

            elf_file
                .write_all(unsafe { struct_to_bytes(&sh_serialised) })
                .unwrap_or_else(|_| {
                    panic!(
                        "Failed to write ELF section header #{} for '{}'",
                        i,
                        out.display()
                    )
                });
        }

        // Finally the data for each segment will follow
        for (i, seg) in self
            .loadable_segments()
            .iter()
            .filter(|seg| !seg.is_uninitialised())
            .enumerate()
        {
            elf_file.write_all(seg.data()).unwrap_or_else(|_| {
                panic!(
                    "Failed to write ELF segment data #{} for '{}'",
                    i,
                    out.display()
                )
            });
        }

        elf_file.flush().unwrap();

        Ok(metadata(out).unwrap().len())
    }
}
