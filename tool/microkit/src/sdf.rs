//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

//! This module is responsible for parsing the System Description Format (SDF)
//! which is based on XML.
//! We do not use any fancy XML, and instead keep things as minimal and simple
//! as possible.
//!
//! As much as possible of the validation of the SDF is done when parsing the XML
//! here.
//!
//! There are various XML parsing/deserialising libraries within the Rust eco-system
//! but few seem to be concerned with giving any introspection regarding the parsed
//! XML. The roxmltree project allows us to work on a lower-level than something based
//! on serde and so we can report proper user errors.

mod channels;
mod consts;
mod cspace;
mod domains;
mod iommu;
mod irq;
mod memory_region;
mod pci;
mod pd_vm;
mod util;

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::sel4::{Arch, Config};
use crate::util::ranges_overlap;
use crate::MAX_PDS;

// Internal imports
use channels::Channel;
use domains::Domains;
use iommu::IOAddressSpace;
use memory_region::{check_io_maps, check_maps, SysIOMap};
use pd_vm::{pd_flatten, IOPort, ProtectionDomain, SysSetVar};
use util::*;

// Internal re-exports
pub(crate) use consts::*;
pub(crate) use cspace::CapMapType;
pub(crate) use iommu::IommuDeviceIdentifier;
pub(crate) use irq::{SysIrq, SysIrqKind};
pub(crate) use memory_region::{Map, SysMapPerms};
pub(crate) use pd_vm::{CpuCore, SysSetVarKind};

// Public re-exports
pub use memory_region::{SysMemoryRegion, SysMemoryRegionPaddr};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SdfLocation {
    pub row: u32,
    pub col: u32,
}

#[derive(Clone, Copy)]
pub struct SdfAttribute<'a> {
    pub name: &'a str,
    pub value: &'a str,
    pub location: SdfLocation,
}

impl<'a, 'input> From<roxmltree::Attribute<'a, 'input>> for SdfAttribute<'a> {
    fn from(attr: roxmltree::Attribute<'a, 'input>) -> Self {
        Self {
            name: attr.name(),
            value: attr.value(),
            location: SdfLocation { row: 0, col: 0 }, // todo
        }
    }
}

/// FIXME: More documentation
/// This (as of 2.3.0-dev) is an experimental interface for use of Microkit as
/// as a library. Please avoid modifying this without justified changes as it
/// will affect external users.
pub trait SdfNode<'a> {
    fn tag_name(&self) -> &str;
    fn attribute(&self, name: &str) -> Option<&str>;
    fn attributes(&self) -> Vec<SdfAttribute<'_>>;
    fn range(&self) -> Range<SdfLocation>;
    fn children(&self) -> Box<dyn Iterator<Item = Box<dyn SdfNode<'a> + 'a>> + 'a>;
}

impl<'a> SdfNode<'a> for roxmltree::Node<'a, '_> {
    fn attributes(&self) -> Vec<SdfAttribute<'_>> {
        self.attributes().map(|attr| attr.into()).collect()
    }

    fn tag_name(&self) -> &str {
        self.tag_name().name()
    }

    fn attribute(&self, name: &str) -> Option<&str> {
        self.attribute(name)
    }

    fn range(&self) -> Range<SdfLocation> {
        let start = self.document().text_pos_at(self.range().start);
        let start = SdfLocation {
            row: start.row,
            col: start.col,
        };
        let end = self.document().text_pos_at(self.range().end);
        let end = SdfLocation {
            row: end.row,
            col: end.col,
        };
        Range { start, end }
    }

    fn children(&self) -> Box<dyn Iterator<Item = Box<dyn SdfNode<'a> + 'a>> + 'a> {
        Box::new(
            self.children()
                .filter(|c| c.is_element())
                .map(|c| Box::new(c) as Box<dyn SdfNode>),
        )
    }
}

pub(crate) struct SystemDescriptionFile<'a> {
    filename: &'a Path,
}

#[derive(Debug)]
pub struct SystemDescription {
    pub protection_domains: HashMap<Rc<str>, ProtectionDomain>,
    pub memory_regions: Vec<SysMemoryRegion>,
    pub iomaps: Vec<SysIOMap>,
    pub channels: Vec<Channel>,
    pub domains: Domains,
}

pub fn parse_xml(
    filename: &Path,
    xml: &str,
    config: &Config,
    search_paths: &Vec<PathBuf>,
) -> Result<SystemDescription, String> {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(doc) => doc,
        Err(err) => return Err(format!("Could not parse '{0}': {err}", filename.display())),
    };

    let xml_sdf = SystemDescriptionFile { filename };

    let system = doc
        .root()
        .children()
        .find(|child| child.tag_name().name() == "system")
        .unwrap();

    // Ensure there is no non-whitespace/comment text
    check_no_text(&xml_sdf, &system)?;

    let system: &dyn SdfNode = &system;

    parse(filename, system, config, search_paths)
}

pub fn parse(
    filename: &Path,
    system: &dyn SdfNode,
    config: &Config,
    search_paths: &Vec<PathBuf>,
) -> Result<SystemDescription, String> {
    let xml_sdf = SystemDescriptionFile { filename };
    let mut root_pds = vec![];
    let mut mrs = vec![];
    let mut iomaps = vec![];
    let mut io_address_space_names = HashSet::new();
    let mut iommu_domain_ids = HashSet::new();
    let mut iommu_device_identifiers = Vec::new();
    let mut channels = vec![];
    let mut domains = Domains::default();

    // Channels cannot be parsed immediately as they refer to a particular protection domain
    // via an index in the list of PDs. This means that we have to parse all PDs first and
    // then parse the channels.
    let mut channel_nodes = Vec::new();

    for child in system.children() {
        let child_name = child.tag_name();
        match child_name {
            "protection_domain" => root_pds.push(ProtectionDomain::from_xml(
                config, &xml_sdf, &*child, false, &domains,
            )?),
            "channel" => channel_nodes.push(child),
            "memory_region" => mrs.push(SysMemoryRegion::from_xml(
                config,
                &xml_sdf,
                &*child,
                search_paths,
            )?),
            "io_address_space" => {
                iomaps.extend(
                    IOAddressSpace::from_xml(
                        config,
                        &xml_sdf,
                        &*child,
                        &mut io_address_space_names,
                        &mut iommu_domain_ids,
                        &mut iommu_device_identifiers,
                    )?
                    .iomaps,
                );
            }
            "virtual_machine" => {
                let pos = child.range().start;
                return Err(format!(
                    "Error: virtual machine must be a child of a protection domain: {}",
                    loc_string(&xml_sdf, pos)
                ));
            }
            "domains" => {
                if domains.has_domains() {
                    return Err(value_error(
                        &xml_sdf,
                        &*child,
                        "domains must only be specified once".to_string(),
                    ));
                }

                domains = Domains::from_xml(config, &xml_sdf, &*child)?;
            }
            _ => {
                let pos = child.range().start;
                return Err(format!(
                    "Error: invalid XML element '{}': {}",
                    child_name,
                    loc_string(&xml_sdf, pos)
                ));
            }
        }
    }

    let pds = pd_flatten(&xml_sdf, root_pds)?;

    // Now that we have parsed everything in the system description we can validate any
    // global properties (e.g no duplicate PD names etc).

    if pds.is_empty() {
        return Err("Error: at least one protection domain must be defined".to_string());
    }

    if pds.len() > MAX_PDS {
        return Err(format!(
            "Error: too many protection domains ({}) defined. Maximum is {}.",
            pds.len(),
            MAX_PDS
        ));
    }

    for pd in pds.iter() {
        if pds.iter().filter(|x| pd.name == x.name).count() > 1 {
            return Err(format!(
                "Error: duplicate protection domain name '{}'.",
                pd.name
            ));
        }
        if &*pd.name == MONITOR_PD_NAME {
            return Err(
                "Error: the PD name 'monitor' is reserved for the Microkit Monitor.".to_string(),
            );
        }
    }

    let mut pds: HashMap<Rc<str>, _> = pds.into_iter().map(|pd| (pd.name.clone(), pd)).collect();

    for node in channel_nodes {
        let ch = Channel::from_xml(&xml_sdf, &*node, &pds)?;

        if let Some(setvar_id) = &ch.end_a.setvar_id {
            let setvar = SysSetVar {
                symbol: setvar_id.to_string(),
                kind: SysSetVarKind::Id { id: ch.end_a.id },
            };
            checked_add_setvar(
                &mut pds.get_mut(&ch.end_a.pd).unwrap().setvars,
                setvar,
                &xml_sdf,
                &*node,
            )?;
        }

        if let Some(setvar_id) = &ch.end_b.setvar_id {
            let setvar = SysSetVar {
                symbol: setvar_id.to_string(),
                kind: SysSetVarKind::Id { id: ch.end_b.id },
            };
            checked_add_setvar(
                &mut pds.get_mut(&ch.end_b.pd).unwrap().setvars,
                setvar,
                &xml_sdf,
                &*node,
            )?;
        }

        channels.push(ch);
    }

    for pd in pds.values() {
        for cap_map in pd.cap_maps.iter() {
            if !pds.contains_key(&cap_map.pd) {
                return Err(format!(
                    "Error: unknown PD name '{}': {}",
                    cap_map.pd,
                    loc_string(&xml_sdf, cap_map.text_pos)
                ));
            };
        }
    }

    for mr in &mrs {
        if mrs.iter().filter(|x| mr.name == x.name).count() > 1 {
            return Err(format!(
                "Error: duplicate memory region name '{}'.",
                mr.name
            ));
        }
    }

    let mut vms: Vec<&str> = vec![];
    for pd in pds.values() {
        if let Some(vm) = &pd.virtual_machine {
            if vms.contains(&vm.name.as_ref()) {
                return Err(format!(
                    "Error: duplicate virtual machine name '{}'.",
                    vm.name
                ));
            }
            vms.push(&vm.name);
        }

        if config.arch == Arch::X86_64 && pd.virtual_machine.is_some() && pd.has_children {
            // When seL4_VMEnter() is called, the kernel only checks the VMM's bound
            // notification for pending signals. Because the endpoint object isn't passed
            // or checked for pending messages, Child PDs won't work while the VCPU is on.
            // Technically, Child PDs could still work when the VCPU is off, but we shouldn't
            // expose this footgun to users.
            return Err(format!(
                    "Error: It is not possible for PD '{}' with a bound vCPU to have children on x86_64: {}",
                    pd.name,
                    loc_string(&xml_sdf, pd.text_pos.unwrap())));
        }
    }

    // Ensure no duplicate IRQs
    let mut all_irqs = Vec::new();
    for pd in pds.values() {
        for sysirq in &pd.irqs {
            if all_irqs.contains(&sysirq.irq_num()) {
                return Err(format!(
                    "Error: duplicate irq: {} in protection domain: '{}' @ {}:{}:{}",
                    sysirq.irq_num(),
                    pd.name,
                    filename.display(),
                    pd.text_pos.unwrap().row,
                    pd.text_pos.unwrap().col
                ));
            }
            all_irqs.push(sysirq.irq_num());
        }
    }

    // Ensure no duplicate channel identifiers.
    // This means checking that no interrupt IDs clash with any channel IDs
    let mut ch_ids = HashMap::with_capacity(pds.len());
    for pd in pds.values() {
        let mut pd_ch_ids = vec![];

        for sysirq in &pd.irqs {
            if pd_ch_ids.contains(&sysirq.id) {
                return Err(format!(
                    "Error: duplicate channel id: {} in protection domain: '{}' @ {}:{}:{}",
                    sysirq.id,
                    pd.name,
                    filename.display(),
                    pd.text_pos.unwrap().row,
                    pd.text_pos.unwrap().col
                ));
            }

            pd_ch_ids.push(sysirq.id);
        }

        ch_ids.insert(&pd.name, pd_ch_ids);
    }

    for ch in &channels {
        if ch_ids[&ch.end_a.pd].contains(&ch.end_a.id) {
            let pd = &pds[&ch.end_a.pd];
            return Err(format!(
                "Error: duplicate channel id: {} in protection domain: '{}' @ {}:{}:{}",
                ch.end_a.id,
                pd.name,
                filename.display(),
                pd.text_pos.unwrap().row,
                pd.text_pos.unwrap().col
            ));
        }

        if ch_ids[&ch.end_b.pd].contains(&ch.end_b.id) {
            let pd = &pds[&ch.end_b.pd];
            return Err(format!(
                "Error: duplicate channel id: {} in protection domain: '{}' @ {}:{}:{}",
                ch.end_b.id,
                pd.name,
                filename.display(),
                pd.text_pos.unwrap().row,
                pd.text_pos.unwrap().col
            ));
        }

        let pd_a = &pds[&ch.end_a.pd];
        let pd_b = &pds[&ch.end_b.pd];
        if ch.end_a.pp && pd_a.priority() >= pd_b.priority() {
            return Err(format!(
                "Error: PPCs must be to protection domains of strictly higher priorities; \
                        channel with PPC exists from pd {} (priority: {}) to pd {} (priority: {})",
                pd_a.name,
                pd_a.priority(),
                pd_b.name,
                pd_b.priority()
            ));
        } else if ch.end_b.pp && pd_b.priority() >= pd_a.priority() {
            return Err(format!(
                "Error: PPCs must be to protection domains of strictly higher priorities; \
                        channel with PPC exists from pd {} (priority: {}) to pd {} (priority: {})",
                pd_b.name,
                pd_b.priority(),
                pd_a.name,
                pd_a.priority()
            ));
        }

        if config.arch == Arch::X86_64
            && ((ch.end_a.pp && pd_b.virtual_machine.is_some())
                || (ch.end_b.pp && pd_a.virtual_machine.is_some()))
        {
            // Same cause as child PD above
            return Err(format!(
                "Error: It is not possible to PPC to PD '{}' with a bound vCPU from PD '{}' on x86_64 @ {}:{}:{}",
                    if ch.end_a.pp {&pd_b.name} else {&pd_a.name},
                    if ch.end_a.pp {&pd_a.name} else {&pd_b.name},
                    filename.display(),
                    pd_a.text_pos.unwrap().row,
                    pd_a.text_pos.unwrap().col
            ));
        }

        ch_ids.get_mut(&ch.end_a.pd).unwrap().push(ch.end_a.id);
        ch_ids.get_mut(&ch.end_b.pd).unwrap().push(ch.end_b.id);
    }

    // Ensure no duplicate I/O Ports
    for pd in pds.values() {
        let mut seen_ioport_ids: Vec<u64> = Vec::new();
        for ioport in &pd.ioports {
            if seen_ioport_ids.contains(&ioport.id) {
                return Err(format!(
                    "Error: duplicate I/O port id: {} in protection domain: '{}' @ {}:{}:{}",
                    ioport.id,
                    pd.name,
                    filename.display(),
                    pd.text_pos.unwrap().row,
                    pd.text_pos.unwrap().col
                ));
            } else {
                seen_ioport_ids.push(ioport.id);
            }
        }
    }

    // Ensure I/O Ports' size are valid and they don't overlap.
    let mut seen_ioports: Vec<(&str, &IOPort)> = Vec::new();
    for pd in pds.values() {
        for this_ioport in &pd.ioports {
            for (seen_pd_name, seen_ioport) in &seen_ioports {
                let left_range = this_ioport.addr..this_ioport.addr + this_ioport.size;
                let right_range = seen_ioport.addr..seen_ioport.addr + seen_ioport.size;
                if ranges_overlap(&left_range, &right_range) {
                    return Err(format!(
                            "Error: I/O port id: {}, half-open range: [{:#x}, {:#x}) in protection domain: '{}' @ {}:{}:{} overlaps with I/O port id: {}, half-open range: [{:#x}, {:#x}) in protection domain: '{}' @ {}:{}:{}",
                            this_ioport.id,
                            left_range.start,
                            left_range.end,
                            pd.name,
                            filename.display(),
                            this_ioport.text_pos.row,
                            this_ioport.text_pos.col,
                            seen_ioport.id,
                            right_range.start,
                            right_range.end,
                            seen_pd_name,
                            filename.display(),
                            seen_ioport.text_pos.row,
                            seen_ioport.text_pos.col
                        ));
                }
            }
            seen_ioports.push((&pd.name, this_ioport));
        }
    }

    // Ensure that all maps are correct
    for pd in pds.values() {
        check_maps(
            &xml_sdf,
            &mrs,
            pd.maps.iter(),
            &format!("protection domain '{}'", pd.name),
            config.pd_map_max_vaddr(pd.stack_size),
        )?;
        if let Some(vm) = &pd.virtual_machine {
            check_maps(
                &xml_sdf,
                &mrs,
                vm.maps.iter(),
                &format!("virtual machine '{}'", vm.name),
                config.vm_map_max_vaddr(),
            )?;
        }
    }

    check_io_maps(&xml_sdf, &mrs, &iomaps)?;

    // Ensure that there are no overlapping extra cap maps in the user caps region
    // and we are not mapping in the same cap from the same source more than once
    for pd in pds.values() {
        let mut user_cap_slots = HashMap::<u64, Vec<_>>::new();

        for cap_map in &pd.cap_maps {
            user_cap_slots
                .entry(cap_map.slot)
                .and_modify(|v| v.push(cap_map))
                .or_insert(vec![cap_map]);
        }

        for (slot, cap_maps) in user_cap_slots.iter() {
            if cap_maps.len() > 1 {
                let mut lines = String::new();
                for mapping in cap_maps {
                    lines.push_str(&format!(
                        "\n  type {:?} from '{}' at '{}'",
                        mapping.cap_type,
                        mapping.pd,
                        loc_string(&xml_sdf, mapping.text_pos)
                    ));
                }
                return Err(format!(
                    "Error: overlapping user caps in slot {slot} of protection domain '{}':{}",
                    pd.name, lines
                ));
            }
        }
    }

    // Ensure MRs with physical addresses do not overlap
    let mut checked_mrs = Vec::with_capacity(mrs.len());
    for mr in &mrs {
        if let SysMemoryRegionPaddr::Specified(sdf_paddr) = mr.phys_addr {
            let mr_start = sdf_paddr;
            let mr_end = sdf_paddr + mr.size;

            for (name, start, end) in &checked_mrs {
                if !(mr_start >= *end || mr_end <= *start) {
                    let pos = mr.text_pos.unwrap();
                    return Err(
                        format!(
                            "Error: memory region '{}' physical address range [{:#x}..{:#x}) overlaps with another memory region '{}' [{:#x}..{:#x}) @ {}",
                            mr.name,
                            mr_start,
                            mr_end,
                            name,
                            start,
                            end,
                            loc_string(&xml_sdf, pos)
                        )
                    );
                }
            }

            checked_mrs.push((&mr.name, mr_start, mr_end));
        }
    }

    // Check that all MRs are used
    let mut all_maps = vec![];
    for pd in pds.values() {
        all_maps.extend(&pd.maps);
        if let Some(vm) = &pd.virtual_machine {
            all_maps.extend(&vm.maps);
        }
    }
    for mr in &mrs {
        let mut found = false;
        for map in &all_maps {
            if map.mr == mr.name {
                found = true;
                break;
            }
        }

        if !found {
            println!("WARNING: unused memory region '{}'", mr.name);
        }
    }

    // Optimise page size of MRs, if the page size is not specified
    for mr in &mut mrs {
        if mr.page_size_specified_by_user || iomaps.iter().any(|iomap| iomap.mr_name() == mr.name) {
            continue;
        }

        // If the largest possible page size based on the MR's size is already
        // set as its page size, skip it.
        let mr_largest_page_size = mr.optimal_page_size(config);
        if mr.page_size_bytes() == mr_largest_page_size {
            continue;
        }

        // Get all the addresses that this MR will be mapped into
        let mut addrs: Vec<_> = all_maps
            .iter()
            .filter_map(|&map| {
                if map.mr == mr.name {
                    Some(map.vaddr)
                } else {
                    None
                }
            })
            .collect();
        if let SysMemoryRegionPaddr::Specified(sdf_paddr) = mr.phys_addr {
            addrs.push(sdf_paddr);
        }

        // Get all page sizes larger than the MR's current one, sorted from
        // largest to smallest
        let larger_page_sizes: Vec<u64> = config
            .page_sizes()
            .into_iter()
            .filter(|page_size| *page_size > mr.page_size_bytes())
            .rev()
            .collect();
        // Go through potential page sizes and check if the alignment is valid
        // on all addresses we're mapping the MR into.
        for larger_page_size in larger_page_sizes {
            if addrs
                .iter()
                .any(|addr| !addr.is_multiple_of(larger_page_size))
            {
                continue;
            }

            // Safe to increase page size
            mr.page_size = larger_page_size.into();
            mr.page_count = mr.size / mr.page_size_bytes();
        }
    }

    // If any MRs are subject of a setvar region_paddr, update its phys_addr field to indicate tool allocated.
    let mut mr_names_with_setvar_paddr = HashSet::new();
    for pd in pds.values() {
        for setvar in pd.setvars.iter() {
            if let SysSetVarKind::Paddr { region } = &setvar.kind {
                mr_names_with_setvar_paddr.insert(region);
            };
            if let SysSetVarKind::PrefillSize { mr } = &setvar.kind {
                for matching_mr in &mrs {
                    if matching_mr.name == *mr && matching_mr.prefill_bytes.is_none() {
                        return Err(format!(
                            "Error: 'setvar_prefill_size' used for MR without a `prefill_path` @ '{}' {}",
                            matching_mr.name,
                            loc_string(&xml_sdf, matching_mr.text_pos.unwrap()),
                        ));
                    }
                }
            }
        }
    }
    for mr in mrs.iter_mut() {
        if mr_names_with_setvar_paddr.contains(&mr.name)
            && mr.phys_addr == SysMemoryRegionPaddr::Unspecified
        {
            match config.arch {
                Arch::Aarch64 | Arch::Riscv64 => {
                    // The actual allocation is done by another part of the tool.
                    mr.phys_addr = SysMemoryRegionPaddr::ToolAllocated(None);
                }
                Arch::X86_64 => {
                    return Err(format!(
                        "Error: setvar with 'region_paddr' for MR without a specified paddr is unsupported on x86-64 @ '{}' {}",
                        mr.name,
                        loc_string(&xml_sdf, mr.text_pos.unwrap()),
                    ));
                }
            };
        }
    }

    Ok(SystemDescription {
        protection_domains: pds,
        memory_regions: mrs,
        iomaps,
        channels,
        domains,
    })
}
