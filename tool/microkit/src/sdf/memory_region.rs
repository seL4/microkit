//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use sel4_capdl_initializer_types::x86_io_address_space;
use sel4_capdl_initializer_types::FillEntryContentBootInfoId;

use super::iommu::IommuDeviceIdentifier;
use super::util::location_suffix_format;
use super::util::{check_attributes, checked_lookup, sdf_parse_number, value_error};
use super::{SdfLocation, SdfNode, XmlSystemDescription};

use crate::util::get_full_path;
use crate::util::round_up;
use crate::util::str_to_bool;
use crate::{Config, PageSize};

#[repr(u8)]
pub enum SysMapPerms {
    Read = 1,
    Write = 2,
    Execute = 4,
}

impl SysMapPerms {
    fn from_str(s: &str) -> Result<u8, ()> {
        let mut perms = 0;
        for c in s.chars() {
            match c {
                'r' => perms |= SysMapPerms::Read as u8,
                'w' => perms |= SysMapPerms::Write as u8,
                'x' => perms |= SysMapPerms::Execute as u8,
                _ => return Err(()),
            }
        }

        Ok(perms)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysMap {
    pub mr: String,
    pub vaddr: u64,
    pub perms: u8,
    pub cached: bool,
    /// Location in the parsed SDF file. Because this struct is
    /// used in a non-XML context, we make the position optional.
    pub text_pos: Option<SdfLocation>,
}

pub trait Map {
    fn mr_name(&self) -> &str;
    fn addr(&self) -> u64;
    fn text_pos(&self) -> Option<SdfLocation>;
    fn element(&self) -> &'static str;
    fn addr_name(&self) -> &'static str;
    fn range_name(&self) -> &'static str;
    fn read(&self) -> bool;
    fn write(&self) -> bool;
    fn execute(&self) -> bool;
    fn cached(&self) -> bool;
}

impl Map for SysMap {
    fn mr_name(&self) -> &str {
        &self.mr
    }

    fn addr(&self) -> u64 {
        self.vaddr
    }

    fn text_pos(&self) -> Option<SdfLocation> {
        self.text_pos
    }

    fn element(&self) -> &'static str {
        "map"
    }

    fn addr_name(&self) -> &'static str {
        "vaddr"
    }

    fn range_name(&self) -> &'static str {
        "virtual address range"
    }

    fn read(&self) -> bool {
        self.perms & SysMapPerms::Read as u8 != 0
    }

    fn write(&self) -> bool {
        self.perms & SysMapPerms::Write as u8 != 0
    }

    fn execute(&self) -> bool {
        self.perms & SysMapPerms::Execute as u8 != 0
    }

    fn cached(&self) -> bool {
        self.cached
    }
}

impl Map for SysIOMap {
    fn mr_name(&self) -> &str {
        &self.mr
    }

    fn addr(&self) -> u64 {
        self.iovaddr
    }

    fn text_pos(&self) -> Option<SdfLocation> {
        self.text_pos
    }

    fn element(&self) -> &'static str {
        "iomap"
    }

    fn addr_name(&self) -> &'static str {
        "iovaddr"
    }

    fn range_name(&self) -> &'static str {
        "io address range"
    }

    fn read(&self) -> bool {
        matches!(self.perms, SysIOMapPerms::Read | SysIOMapPerms::ReadWrite)
    }

    fn write(&self) -> bool {
        matches!(self.perms, SysIOMapPerms::Write | SysIOMapPerms::ReadWrite)
    }

    fn execute(&self) -> bool {
        false
    }

    fn cached(&self) -> bool {
        false
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum SysMemoryRegionKind {
    User,
    Elf,
    Stack,
    BootInfo,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum SysMemoryRegionPaddr {
    Unspecified,
    // ToolAllocated means that the MR doesn't have an explicit paddr in SDF, but
    // is a subject of a setvar region_paddr.
    ToolAllocated(Option<u64>),
    Specified(u64),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SysMemoryRegion {
    pub name: String,
    pub size: u64,
    pub page_size_specified_by_user: bool,
    pub page_size: PageSize,
    pub page_count: u64,
    pub phys_addr: SysMemoryRegionPaddr,
    pub text_pos: Option<SdfLocation>,
    /// For error reporting is useful to know whether the MR was created
    /// due to the user's SDF or created by the tool for setting up the
    /// stack, ELF, etc.
    pub kind: SysMemoryRegionKind,
    pub prefill_bytes: Option<Vec<u8>>,
    pub prefill_bootinfo: Option<FillEntryContentBootInfoId>,
}

impl SysMemoryRegion {
    /// Given the size of a memory region, returns the 'most optimal'
    /// page size for the platform based on the alignment of the size.
    pub fn optimal_page_size(&self, config: &Config) -> u64 {
        let page_sizes = config.page_sizes();
        for i in (0..page_sizes.len()).rev() {
            if self.size.is_multiple_of(page_sizes[i]) {
                return page_sizes[i];
            }
        }

        panic!("Internal error: size is not aligned to minimum page size");
    }

    pub fn page_size_bytes(&self) -> u64 {
        self.page_size as u64
    }

    pub fn paddr(&self) -> Option<u64> {
        match self.phys_addr {
            SysMemoryRegionPaddr::Unspecified => None,
            SysMemoryRegionPaddr::ToolAllocated(paddr_maybe) => paddr_maybe,
            SysMemoryRegionPaddr::Specified(sdf_paddr) => Some(sdf_paddr),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SysIOMapPerms {
    Read,
    Write,
    ReadWrite,
}

impl SysIOMapPerms {
    fn from_str(s: &str) -> Result<Self, ()> {
        let mut read = false;
        let mut write = false;

        for c in s.chars() {
            match c {
                'r' => read = true,
                'w' => write = true,
                _ => return Err(()),
            }
        }

        match (read, write) {
            (true, true) => Ok(SysIOMapPerms::ReadWrite),
            (true, false) => Ok(SysIOMapPerms::Read),
            (false, true) => Ok(SysIOMapPerms::Write),
            (false, false) => Err(()),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SysIOMap {
    pub name: String,
    pub mr: String,
    pub identifier: IommuDeviceIdentifier,
    pub domain_id: Option<u64>,
    pub iovaddr: u64,
    pub perms: SysIOMapPerms,
    pub text_pos: Option<SdfLocation>,
}

impl SysMap {
    pub(super) fn from_xml(
        xml_sdf: &XmlSystemDescription,
        node: &dyn SdfNode,
        allow_setvar: bool,
        max_vaddr: u64,
    ) -> Result<SysMap, String> {
        let mut attrs = vec!["mr", "vaddr", "perms", "cached"];
        if allow_setvar {
            attrs.push("setvar_vaddr");
            attrs.push("setvar_size");
            attrs.push("setvar_prefill_size");
        }
        check_attributes(xml_sdf, node, &attrs)?;

        let mr = checked_lookup(xml_sdf, node, "mr")?.to_string();
        let vaddr = sdf_parse_number(checked_lookup(xml_sdf, node, "vaddr")?, node)?;

        if vaddr >= max_vaddr {
            return Err(value_error(
                xml_sdf,
                node,
                format!("vaddr ({vaddr:#x}) must be less than {max_vaddr:#x}"),
            ));
        }

        let perms = if let Some(xml_perms) = node.attribute("perms") {
            match SysMapPerms::from_str(xml_perms) {
                Ok(parsed_perms) => parsed_perms,
                Err(()) => {
                    return Err(value_error(
                        xml_sdf,
                        node,
                        "perms must only be a combination of 'r', 'w', and 'x'".to_string(),
                    ))
                }
            }
        } else {
            // Default to read-write
            SysMapPerms::Read as u8 | SysMapPerms::Write as u8
        };

        // On all architectures, the kernel does not allow write-only mappings
        if perms == SysMapPerms::Write as u8 {
            return Err(value_error(
                xml_sdf,
                node,
                "perms must not be 'w', write-only mappings are not allowed".to_string(),
            ));
        }

        let cached = if let Some(xml_cached) = node.attribute("cached") {
            match str_to_bool(xml_cached) {
                Some(val) => val,
                None => {
                    return Err(value_error(
                        xml_sdf,
                        node,
                        "cached must be 'true' or 'false'".to_string(),
                    ))
                }
            }
        } else {
            // Default to cached
            true
        };

        Ok(SysMap {
            mr,
            vaddr,
            perms,
            cached,
            text_pos: Some(node.range().start),
        })
    }
}

impl SysIOMap {
    pub(super) fn from_xml(
        _config: &Config,
        xml_sdf: &XmlSystemDescription,
        node: &dyn SdfNode,
        name: &str,
        identifier: IommuDeviceIdentifier,
        domain_id: Option<u64>,
    ) -> Result<SysIOMap, String> {
        let attrs = vec!["mr", "iovaddr", "perms"];

        check_attributes(xml_sdf, node, &attrs)?;

        let mr = checked_lookup(xml_sdf, node, "mr")?.to_string();
        let iovaddr = sdf_parse_number(checked_lookup(xml_sdf, node, "iovaddr")?, node)?;

        if iovaddr > x86_io_address_space::CAPDL_MAX_IOVA {
            return Err(value_error(
                xml_sdf,
                node,
                format!(
                    "iovaddr ({iovaddr:#x}) must be less than {:#x}",
                    x86_io_address_space::CAPDL_MAX_IOVA + 1
                ),
            ));
        }

        let perms = if let Some(xml_perms) = node.attribute("perms") {
            match SysIOMapPerms::from_str(xml_perms) {
                Ok(parsed_perms) => parsed_perms,
                Err(()) => {
                    return Err(value_error(
                        xml_sdf,
                        node,
                        "perms for io mapped memory must only be a combination of 'r' and 'w'"
                            .to_string(),
                    ))
                }
            }
        } else {
            // Default to read-write
            SysIOMapPerms::ReadWrite
        };

        Ok(SysIOMap {
            name: name.to_string(),
            mr,
            identifier,
            domain_id,
            iovaddr,
            perms,
            text_pos: Some(node.range().start),
        })
    }
}

impl SysMemoryRegion {
    fn determine_size(
        xml_sdf: &XmlSystemDescription,
        node: &dyn SdfNode,
        prefill_bytes_maybe: &Option<Vec<u8>>,
        prefill_bootinfo_maybe: Option<FillEntryContentBootInfoId>,
        page_size: u64,
    ) -> Result<u64, String> {
        match checked_lookup(xml_sdf, node, "size") {
            Ok(size_str) => {
                // Size explicitly specified
                let size_parsed = sdf_parse_number(size_str, node)?;

                if !size_parsed.is_multiple_of(page_size) {
                    return Err(value_error(
                        xml_sdf,
                        node,
                        "size is not a multiple of the page size".to_string(),
                    ));
                }

                match &prefill_bytes_maybe {
                    Some(bytes) => {
                        if bytes.len() > size_parsed as usize {
                            return Err(value_error(
                                xml_sdf,
                                node,
                                format!(
                                    "size of prefill file exceeds memory region size: {:x} > {:x}",
                                    bytes.len(),
                                    size_parsed
                                ),
                            ));
                        }

                        Ok(size_parsed)
                    }
                    None => Ok(size_parsed),
                }
            }

            Err(_) => {
                if prefill_bootinfo_maybe.is_some() {
                    Ok(page_size)
                } else {
                    // No size explicitly specified
                    match &prefill_bytes_maybe {
                        Some(bytes) => Ok(round_up(bytes.len() as u64, page_size)),

                        None => Err(value_error(
                            xml_sdf,
                            node,
                            "size must be specified if memory region is not prefilled".to_string(),
                        )),
                    }
                }
            }
        }
    }

    pub(super) fn from_xml(
        config: &Config,
        xml_sdf: &XmlSystemDescription,
        node: &dyn SdfNode,
        search_paths: &Vec<PathBuf>,
    ) -> Result<SysMemoryRegion, String> {
        check_attributes(
            xml_sdf,
            node,
            &[
                "name",
                "size",
                "page_size",
                "phys_addr",
                "prefill_path",
                "prefill_bootinfo",
            ],
        )?;

        let name = checked_lookup(xml_sdf, node, "name")?;

        let mut page_size_specified_by_user = false;
        let page_size = if let Some(xml_page_size) = node.attribute("page_size") {
            page_size_specified_by_user = true;
            sdf_parse_number(xml_page_size, node)?
        } else {
            config.page_sizes()[0]
        };

        let page_size_valid = config.page_sizes().contains(&page_size);
        if !page_size_valid {
            return Err(value_error(
                xml_sdf,
                node,
                format!("page size {page_size:#x} not supported"),
            ));
        }

        let prefill_bytes_maybe = node
            .attribute("prefill_path")
            .map(|path_str| {
                get_full_path(&PathBuf::from(path_str), search_paths)
                    .ok_or_else(|| {
                        value_error(
                            xml_sdf,
                            node,
                            format!("unable to find prefill file: '{path_str}'"),
                        )
                    })
                    .and_then(|prefill_path| {
                        fs::read(&prefill_path)
                            .map_err(|_| {
                                value_error(
                                    xml_sdf,
                                    node,
                                    format!("failed to read file '{path_str}' at prefill_path"),
                                )
                            })
                            .and_then(|bytes| {
                                if bytes.is_empty() {
                                    Err(value_error(
                                        xml_sdf,
                                        node,
                                        format!("prefill file '{path_str}' is empty"),
                                    ))
                                } else {
                                    Ok(bytes)
                                }
                            })
                    })
            })
            .transpose()?;

        let prefill_bootinfo_maybe = node
            .attribute("prefill_bootinfo")
            .map(|xml_bi_type| match xml_bi_type {
                "x86_vbe" => Ok(FillEntryContentBootInfoId::X86Vbe),
                "x86_mbmmap" => Ok(FillEntryContentBootInfoId::X86Mbmmap),
                "x86_acpi_rsdp" => Ok(FillEntryContentBootInfoId::X86AcpiRsdp),
                "x86_framebuffer" => Ok(FillEntryContentBootInfoId::X86FrameBuffer),
                "x86_tsc_freq" => Ok(FillEntryContentBootInfoId::X86TscFreq),
                "fdt" => Ok(FillEntryContentBootInfoId::Fdt),
                _ => Err(value_error(
                    xml_sdf,
                    node,
                    format!("BootInfoMap type: '{xml_bi_type}' is not supported"),
                )),
            })
            .transpose()?;

        if prefill_bytes_maybe.is_some() && prefill_bootinfo_maybe.is_some() {
            return Err(value_error(
                xml_sdf,
                node,
                "prefill_path and prefill_bootinfo cannot be both specified".to_string(),
            ));
        }

        let mr_kind = if prefill_bootinfo_maybe.is_none() {
            SysMemoryRegionKind::User
        } else {
            SysMemoryRegionKind::BootInfo
        };

        let size = Self::determine_size(
            xml_sdf,
            node,
            &prefill_bytes_maybe,
            prefill_bootinfo_maybe,
            page_size,
        )?;

        let phys_addr = if let Some(xml_phys_addr) = node.attribute("phys_addr") {
            SysMemoryRegionPaddr::Specified(sdf_parse_number(xml_phys_addr, node)?)
        } else {
            // At this point it is unsure whether this MR is a subject of a setvar region_paddr.
            SysMemoryRegionPaddr::Unspecified
        };

        if let SysMemoryRegionPaddr::Specified(sdf_paddr) = phys_addr {
            if !sdf_paddr.is_multiple_of(page_size) {
                return Err(value_error(
                    xml_sdf,
                    node,
                    "phys_addr is not aligned to the page size".to_string(),
                ));
            }
        }

        let page_count = size / page_size;

        Ok(SysMemoryRegion {
            name: name.to_string(),
            size,
            page_size: page_size.into(),
            page_size_specified_by_user,
            page_count,
            phys_addr,
            text_pos: Some(node.range().start),
            kind: mr_kind,
            prefill_bytes: prefill_bytes_maybe,
            prefill_bootinfo: prefill_bootinfo_maybe,
        })
    }
}

// max_end is the first invalid virtual address
pub fn check_maps<'a, M, I>(
    xml_sdf: &XmlSystemDescription,
    mrs: &[SysMemoryRegion],
    maps: I,
    address_space: &str,
    max_end: u64,
) -> Result<(), String>
where
    M: Map + 'a,
    I: IntoIterator<Item = &'a M>,
{
    let mut checked_maps: Vec<(&str, u64, u64)> = Vec::new();

    for map in maps {
        let element = map.element();
        match mrs.iter().find(|mr| mr.name == map.mr_name()) {
            Some(mr) => {
                if !map.addr().is_multiple_of(mr.page_size_bytes()) {
                    return Err(format!(
                        "Error: invalid {} alignment on '{element}' {}",
                        map.addr_name(),
                        location_suffix_format(xml_sdf, map.text_pos())
                    ));
                }

                let map_start = map.addr();
                let Some(map_end) = map_start.checked_add(mr.size) else {
                    return Err(format!(
                        "Error: {element} for '{}' has address range that overflows {}",
                        map.mr_name(),
                        location_suffix_format(xml_sdf, map.text_pos())
                    ));
                };

                if map_end > max_end {
                    return Err(format!(
                        "Error: {element} for '{}' has {} [{:#x}..{:#x}) which exceeds valid address space [{:#x}..{:#x}) {}",
                        map.mr_name(),
                        map.range_name(),
                        map_start,
                        map_end,
                        0,
                        max_end,
                        location_suffix_format(xml_sdf, map.text_pos())
                    ));
                }

                for (name, start, end) in checked_maps.iter() {
                    if !(map_start >= *end || map_end <= *start) {
                        return Err(format!(
                            "Error: map for '{}' has {} [{:#x}..{:#x}) which overlaps with map for '{}' [{:#x}..{:#x}) in {} {}",
                            map.mr_name(),
                            map.range_name(),
                            map_start,
                            map_end,
                            name,
                            start,
                            end,
                            address_space,
                            location_suffix_format(xml_sdf, map.text_pos())
                        ));
                    }
                }
                checked_maps.push((map.mr_name(), map_start, map_end));
            }
            None => {
                return Err(format!(
                    "Error: invalid memory region name '{}' on '{element}' {}",
                    map.mr_name(),
                    location_suffix_format(xml_sdf, map.text_pos())
                ));
            }
        }
    }

    Ok(())
}

pub fn check_io_maps(
    xml_sdf: &XmlSystemDescription,
    mrs: &[SysMemoryRegion],
    iomaps: &[SysIOMap],
) -> Result<(), String> {
    let mut by_device: HashMap<&str, Vec<&SysIOMap>> = HashMap::new();

    for iomap in iomaps {
        by_device
            .entry(iomap.name.as_str())
            .or_default()
            .push(iomap);
    }

    if iomaps.iter().any(|iomap| {
        mrs.iter()
            .any(|mr| mr.page_size == PageSize::Large && mr.name == iomap.mr_name())
    }) {
        return Err(
            "Error: currently seL4 does not have large page support for the IOMMU".to_string(),
        );
    }

    for maps in by_device.into_values() {
        let last = maps.iter().last().unwrap();
        let address_space = last.identifier.to_string();
        check_maps(
            xml_sdf,
            mrs,
            maps,
            &address_space,
            x86_io_address_space::CAPDL_MAX_IOVA + 1,
        )?;
    }

    Ok(())
}
