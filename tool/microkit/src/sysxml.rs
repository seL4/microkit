//
// Copyright 2024, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::path::{Path, PathBuf};
use crate::util::{str_to_bool};
use crate::sel4::{PageSize, ArmIrqTrigger, Config, Arch};
use crate::MAX_PDS;

///
/// This module is responsible for parsing the System Description Format (SDF)
/// which is based on XML.
/// We do not use any fancy XML, and instead keep things as minimal and simple
/// as possible.
///
/// As much as possible of the validation of the SDF is done when parsing the XML
/// here.
///
/// There are various XML parsing/deserialising libraries within the Rust eco-system
/// but few seem to be concerned with giving any introspection regarding the parsed
/// XML. The roxmltree project allows us to work on a lower-level than something based
/// on serde and so we can report proper user errors.
///

/// Events that come through entry points (e.g notified or protected) are given an
/// identifier that is used as the badge at runtime.
/// On 64-bit platforms, this badge has a limit of 64-bits which means that we are
/// limited in how many IDs a Microkit protection domain has since each ID represents
/// a unique bit.
/// Currently the first bit is used to determine whether or not the event is a PPC
/// or notification. The second bit is used to determine whether a fault occurred.
/// This means we are left with 62 bits for the ID.
/// IDs start at zero.
const PD_MAX_ID: u64 = 61;

const PD_MAX_PRIORITY: u8 = 254;

/// The purpose of this function is to parse an integer that could
/// either be in decimal or hex format, unlike the normal parsing
/// functionality that the Rust standard library provides.
/// This also removes any underscores that may be present in the number
/// Always returns a base 10 integer.
fn sdf_parse_number(s: &str, node: &roxmltree::Node) -> Result<u64, String> {
    let mut to_parse = s.to_string();
    to_parse.retain(|c| c != '_');

    let (final_str, base) = match to_parse.strip_prefix("0x") {
        Some(stripped) => (stripped, 16),
        None => (to_parse.as_str(), 10),
    };

    match u64::from_str_radix(final_str, base) {
        Ok(value) => Ok(value),
        Err(err) => Err(format!("Error: failed to parse integer '{}' on element '{}': {}", s, node.tag_name().name(), err))
    }
}

fn loc_string(xml_sdf: &XmlSystemDescription, pos: roxmltree::TextPos) -> String {
    format!("{}:{}:{}", xml_sdf.filename, pos.row, pos.col)
}

/// There are some platform-specific properties that must be known when parsing the
/// SDF for error-checking and validation, these go in this struct.
pub struct PlatformDescription {
    /// Note that we have the invariant that page sizes are be ordered by size
    page_sizes: [u64; 2],
}

impl PlatformDescription {
    pub const fn new(kernel_config: &Config) -> PlatformDescription {
        let page_sizes = match kernel_config.arch {
            Arch::Aarch64 => [0x1000, 0x200_000],
        };

        PlatformDescription {
            page_sizes,
        }
    }
}

#[repr(u8)]
pub enum SysMapPerms {
    Read = 1,
    Write = 2,
    Execute = 4,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SysMap {
    pub mr: String,
    pub vaddr: u64,
    pub perms: u8,
    pub cached: bool,
    /// Location in the parsed SDF file. Because this struct is
    /// used in a non-XML context, we make the position optional.
    pub text_pos: Option<roxmltree::TextPos>,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct SysMemoryRegion {
    pub name: String,
    pub size: u64,
    pub page_size: PageSize,
    pub page_count: u64,
    pub phys_addr: Option<u64>,
}

impl SysMemoryRegion {
    pub fn page_bytes(&self) -> u64 {
        self.page_size as u64
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct SysIrq {
    pub irq: u64,
    pub id: u64,
    pub trigger: ArmIrqTrigger,
}

// The use of SysSetVar depends on the context. In some
// cases it will contain a symbol and a physical or a
// symbol and vaddr. Never both.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct SysSetVar {
    pub symbol: String,
    pub region_paddr: Option<String>,
    pub vaddr: Option<u64>,
}

#[derive(Debug)]
pub struct Channel {
    pub pd_a: usize,
    pub id_a: u64,
    pub pd_b: usize,
    pub id_b: u64,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ProtectionDomain {
    /// Only populated for child protection domains
    pub id: Option<u64>,
    pub name: String,
    pub priority: u8,
    pub budget: u64,
    pub period: u64,
    pub pp: bool,
    pub passive: bool,
    pub program_image: PathBuf,
    pub maps: Vec<SysMap>,
    pub irqs: Vec<SysIrq>,
    pub setvars: Vec<SysSetVar>,
    /// Only used when parsing child PDs. All elements will be removed
    /// once we flatten each PD and its children into one list.
    pub child_pds: Vec<ProtectionDomain>,
    pub has_children: bool,
    /// Index into the total list of protection domains if a parent
    /// protection domain exists
    pub parent: Option<usize>,
    /// Location in the parsed SDF file
    text_pos: roxmltree::TextPos
}

impl SysMapPerms {
    fn from_str(s: &str) -> Result<u8, &'static str> {
        let mut perms = 0;
        for c in s.chars() {
            match c {
                'r' => perms |= SysMapPerms::Read as u8,
                'w' => perms |= SysMapPerms::Write as u8,
                'x' => perms |= SysMapPerms::Execute as u8,
                _ => return Err("invalid permissions")
            }
        }

        Ok(perms)
    }
}

impl ProtectionDomain {
    pub fn needs_ep(&self) -> bool {
        self.pp || self.has_children
    }

    fn from_xml(xml_sdf: &XmlSystemDescription, node: &roxmltree::Node, is_child: bool) -> Result<ProtectionDomain, String> {
        let mut attrs = vec!["name", "priority", "pp", "budget", "period", "passive"];
        if is_child {
            attrs.push("id");
        }
        check_attributes(xml_sdf, node, &attrs)?;

        let name = checked_lookup(xml_sdf, node, "name")?.to_string();

        let id = if is_child {
            Some(sdf_parse_number(checked_lookup(xml_sdf, node, "id")?, node)?)
        } else {
            None
        };

        // Default to 1000 microseconds as the budget, with the period defaulting
        // to being the same as the budget as well.
        let budget = if let Some(xml_budget) = node.attribute("budget") {
            sdf_parse_number(xml_budget, node)?
        } else {
            1000
        };
        let period = if let Some(xml_period) = node.attribute("period") {
            sdf_parse_number(xml_period, node)?
        } else {
            budget
        };
        if budget > period {
            return Err(value_error(xml_sdf, node, format!("budget ({}) must be less than, or equal to, period ({})", budget, period)));
        }

        let pp = if let Some(xml_pp) = node.attribute("pp") {
            str_to_bool(xml_pp)?
        } else {
            false
        };

        let passive = if let Some(xml_passive) = node.attribute("passive") {
            str_to_bool(xml_passive)?
        } else {
            false
        };

        let mut maps = Vec::new();
        let mut irqs = Vec::new();
        let mut setvars = Vec::new();
        let mut child_pds = Vec::new();

        let mut program_image = None;

        // Default to minimum priority
        let priority = if let Some(xml_priority) = node.attribute("priority") {
            sdf_parse_number(xml_priority, node)?
        } else {
            0
        };

        if priority > PD_MAX_PRIORITY as u64 {
            return Err(value_error(xml_sdf, node, format!("priority must be between 0 and {}", PD_MAX_PRIORITY)));
        }

        for child in node.children() {
            if !child.is_element() {
                continue;
            }

            match child.tag_name().name() {
                "program_image" => {
                    check_attributes(xml_sdf, &child, &["path"])?;
                    if program_image.is_some() {
                        return Err(value_error(xml_sdf, node, "program_image must only be specified once".to_string()));
                    }

                    let program_image_path = checked_lookup(xml_sdf, &child, "path")?;
                    program_image = Some(Path::new(program_image_path).to_path_buf());
                },
                "map" => {
                    check_attributes(xml_sdf, &child, &["mr", "vaddr", "perms", "cached", "setvar_vaddr"])?;
                    let mr = checked_lookup(xml_sdf, &child, "mr")?.to_string();
                    let vaddr = sdf_parse_number(checked_lookup(xml_sdf, &child, "vaddr")?, &child)?;
                    let perms = if let Some(xml_perms) = child.attribute("perms") {
                        SysMapPerms::from_str(xml_perms)?
                    } else {
                        // Default to read-write
                        SysMapPerms::Read as u8 | SysMapPerms::Write as u8
                    };

                    // On all architectures, the kernel does not allow write-only mappings
                    if perms == SysMapPerms::Write as u8 {
                        return Err(value_error(xml_sdf, &child, "perms must not be 'w', write-only mappings are not allowed".to_string()));
                    }

                    let cached = if let Some(xml_cached) = child.attribute("cached") {
                        str_to_bool(xml_cached)?
                    } else {
                        // Default to cached
                        true
                    };

                    maps.push(SysMap {
                        mr,
                        vaddr,
                        perms,
                        cached,
                        text_pos: Some(xml_sdf.doc.text_pos_at(child.range().start)),
                    });

                    if let Some(setvar_vaddr) = child.attribute("setvar_vaddr") {
                        setvars.push(SysSetVar {
                            symbol: setvar_vaddr.to_string(),
                            region_paddr: None,
                            vaddr: Some(vaddr),
                        });
                    }
                }
                "irq" => {
                    check_attributes(xml_sdf, &child, &["irq", "id", "trigger"])?;
                    let irq = checked_lookup(xml_sdf, &child, "irq")?.parse::<u64>().unwrap();
                    let id = checked_lookup(xml_sdf, &child, "id")?.parse::<i64>().unwrap();
                    if id > PD_MAX_ID as i64 {
                        return Err(value_error(xml_sdf, &child, format!("id must be < {}", PD_MAX_ID + 1)));
                    }
                    if id < 0 {
                        return Err(value_error(xml_sdf, &child, "id must be >= 0".to_string()));
                    }

                    let trigger = if let Some(trigger_str) = child.attribute("trigger") {
                        match trigger_str {
                            "level" => ArmIrqTrigger::Level,
                            "edge" => ArmIrqTrigger::Edge,
                            _ => return Err(value_error(xml_sdf, &child, "trigger must be either 'level' or 'edge'".to_string()))
                        }
                    } else {
                        // Default the level triggered
                        ArmIrqTrigger::Level
                    };

                    let irq = SysIrq {
                        irq,
                        id: id as u64,
                        trigger
                    };
                    irqs.push(irq);
                },
                "setvar" => {
                    check_attributes(xml_sdf, &child, &["symbol", "region_paddr"])?;
                    let symbol = checked_lookup(xml_sdf, &child, "symbol")?.to_string();
                    let region_paddr = Some(checked_lookup(xml_sdf, &child, "region_paddr")?.to_string());
                    setvars.push(SysSetVar {
                        symbol,
                        region_paddr,
                        vaddr: None,
                    })
                },
                "protection_domain" => child_pds.push(ProtectionDomain::from_xml(xml_sdf, &child, true)?),
                _ => {
                    let pos = xml_sdf.doc.text_pos_at(child.range().start);
                    return Err(format!("Invalid XML element '{}': {}", child.tag_name().name(), loc_string(xml_sdf, pos)));
                },
            }
        }

        if program_image.is_none() {
            return Err(format!("Error: missing 'program_image' element on protection_domain: '{}'", name));
        }

        let has_children = !child_pds.is_empty();

        Ok(ProtectionDomain {
            id,
            name,
            // This downcast is safe as we have checked that this is less than
            // the maximum PD priority, which fits in a u8.
            priority: priority as u8,
            budget,
            period,
            pp,
            passive,
            program_image: program_image.unwrap(),
            maps,
            irqs,
            setvars,
            child_pds,
            has_children,
            parent: None,
            text_pos: xml_sdf.doc.text_pos_at(node.range().start),
        })
    }
}

impl SysMemoryRegion {
    fn from_xml(xml_sdf: &XmlSystemDescription, node: &roxmltree::Node, plat_desc: &PlatformDescription) -> Result<SysMemoryRegion, String> {
        check_attributes(xml_sdf, node, &["name", "size", "page_size", "phys_addr"])?;

        let name = checked_lookup(xml_sdf, node, "name")?;
        let size = sdf_parse_number(checked_lookup(xml_sdf, node, "size")?, node)?;

        let page_size = if let Some(xml_page_size) = node.attribute("page_size") {
            sdf_parse_number(xml_page_size, node)?
        } else {
            // Default to the minimum page size
            plat_desc.page_sizes[0]
        };

        let page_size_valid = plat_desc.page_sizes.contains(&page_size);
        if !page_size_valid {
            return Err(value_error(xml_sdf, node, format!("page size 0x{:x} not supported", page_size)));
        }

        if size % page_size != 0 {
            return Err(value_error(xml_sdf, node, "size is not a multiple of the page size".to_string()));
        }

        let phys_addr = if let Some(xml_phys_addr) = node.attribute("phys_addr") {
            Some(sdf_parse_number(xml_phys_addr, node)?)
        } else {
            None
        };

        if phys_addr.is_some() && phys_addr.unwrap() % page_size != 0 {
            return Err(value_error(xml_sdf, node, "phys_addr is not aligned to the page size".to_string()));
        }

        let page_count = size / page_size;

        Ok(SysMemoryRegion {
            name: name.to_string(),
            size,
            page_size: page_size.into(),
            page_count,
            phys_addr,
        })
    }
}

impl Channel {
    /// It should be noted that this function assumes that `pds` is populated
    /// with all the Protection Domains that could potentially be connected with
    /// the channel.
    fn from_xml<'a>(xml_sdf: &'a XmlSystemDescription, node: &'a roxmltree::Node, pds: &[ProtectionDomain]) -> Result<Channel, String> {
        check_attributes(xml_sdf, node, &[])?;

        let mut ends: Vec<(usize, u64)> = Vec::new();
        for child in node.children() {
            if !child.is_element() {
                continue;
            }

            let child_name = child.tag_name().name();
            match child_name {
                "end" => {
                    check_attributes(xml_sdf, &child, &["pd", "id"])?;
                    let end_pd = checked_lookup(xml_sdf, &child, "pd")?;
                    let end_id = checked_lookup(xml_sdf, &child, "id")?.parse::<i64>().unwrap();

                    if end_id > PD_MAX_ID as i64 {
                        return Err(value_error(xml_sdf, &child, format!("id must be < {}", PD_MAX_ID + 1)));
                    }

                    if end_id < 0 {
                        return Err(value_error(xml_sdf, &child, "id must be >= 0".to_string()));
                    }

                    if let Some(pd_idx) = pds.iter().position(|pd| pd.name == end_pd) {
                        ends.push((pd_idx, end_id as u64))
                    } else {
                        return Err(value_error(xml_sdf, &child, format!("invalid PD name '{end_pd}'")));
                    }
                },
                _ => {
                    let pos = xml_sdf.doc.text_pos_at(node.range().start);
                    return Err(format!("Error: invalid XML element '{}': {}", child_name, loc_string(xml_sdf, pos)));
                }
            }
        }

        if ends.len() != 2 {
            return Err(value_error(xml_sdf, node, "exactly two end elements must be specified".to_string()));
        }

        let (pd_a, id_a) = ends[0];
        let (pd_b, id_b) = ends[1];

        Ok(Channel {
            pd_a,
            id_a,
            pd_b,
            id_b,
        })
    }
}

struct XmlSystemDescription<'a> {
    filename: &'a str,
    doc: &'a roxmltree::Document<'a>,
}

#[derive(Debug)]
pub struct SystemDescription {
    pub protection_domains: Vec<ProtectionDomain>,
    pub memory_regions: Vec<SysMemoryRegion>,
    pub channels: Vec<Channel>,
}

fn check_attributes(xml_sdf: &XmlSystemDescription, node: &roxmltree::Node, attributes: &[&'static str]) -> Result<(), String> {
    for attribute in node.attributes() {
        if !attributes.contains(&attribute.name()) {
            return Err(value_error(xml_sdf, node, format!("invalid attribute '{}'", attribute.name())));
        }
    }

    Ok(())
}

fn checked_lookup<'a>(xml_sdf: &XmlSystemDescription, node: &'a roxmltree::Node, attribute: &'static str) -> Result<&'a str, String> {
    if let Some(value) = node.attribute(attribute) {
        Ok(value)
    } else {
        let pos = xml_sdf.doc.text_pos_at(node.range().start);
        Err(format!("Error: Missing required attribute '{}' on element '{}': {}:{}:{}", attribute, node.tag_name().name(),xml_sdf.filename, pos.row, pos.col))
    }
}

fn value_error(xml_sdf: &XmlSystemDescription, node: &roxmltree::Node, err: String) -> String {
    let pos = xml_sdf.doc.text_pos_at(node.range().start);
    format!("Error: {} on element '{}': {}:{}:{}", err, node.tag_name().name(), xml_sdf.filename, pos.row, pos.col)
}

fn check_no_text(xml_sdf: &XmlSystemDescription, node: &roxmltree::Node) -> Result<(), String> {
    let name = node.tag_name().name();
    let pos = xml_sdf.doc.text_pos_at(node.range().start);

    if let Some(text) = node.text() {
        // If the text is just whitespace then it is okay
        if !text.trim().is_empty() {
            return Err(format!("Error: unexpected text found in element '{}' @ {}", name, loc_string(xml_sdf, pos)));
        }
    }

    if node.tail().is_some() {
        return Err(format!("Error: unexpected text found after element '{}' @ {}", name, loc_string(xml_sdf, pos)));
    }

    for child in node.children() {
        if !child.is_comment() && !child.is_element() {
            check_no_text(xml_sdf, &child)?;
        }
    }

    Ok(())
}

fn pd_tree_to_list(xml_sdf: &XmlSystemDescription, mut root_pd: ProtectionDomain, parent: bool, idx: usize) -> Result<Vec<ProtectionDomain>, String> {
    let mut child_ids = vec![];
    for child_pd in &root_pd.child_pds {
        let child_id = child_pd.id.unwrap();
        if child_ids.contains(&child_id) {
            return Err(format!("Error: duplicate id: {} in protection domain: '{}' @ {}", child_id, root_pd.name, loc_string(xml_sdf, child_pd.text_pos)));
        }
        child_ids.push(child_pd.id.unwrap());
    }

    if parent {
        root_pd.parent = Some(idx);
    } else {
        root_pd.parent = None;
    }
    let mut new_child_pds = vec![];
    let child_pds: Vec<_> = root_pd.child_pds.drain(0..).collect();
    for child_pd in child_pds {
        new_child_pds.extend(pd_tree_to_list(xml_sdf, child_pd, true, idx)?);
    }

    let mut all = vec![root_pd];
    all.extend(new_child_pds);

    Ok(all)
}

/// Given an iterable of protection domains flatten the tree representation
/// into a flat tuple.
///
/// In doing so the representation is changed from "Node with list of children",
/// to each node having a parent link instead.
fn pd_flatten(xml_sdf: &XmlSystemDescription, pds: Vec<ProtectionDomain>) -> Result<Vec<ProtectionDomain>, String> {
    let mut all_pds = vec![];

    for pd in pds {
        all_pds.extend(pd_tree_to_list(xml_sdf, pd, false, 0)?);
    }

    Ok(all_pds)
}

pub fn parse(filename: &str, xml: &str, plat_desc: &PlatformDescription) -> Result<SystemDescription, String> {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(doc) => doc,
        Err(err) => return Err(format!("Could not parse '{}': {}", filename, err))
    };

    let xml_sdf = XmlSystemDescription {
        filename,
        doc: &doc,
    };

    let mut root_pds = vec![];
    let mut mrs = vec![];
    let mut channels = vec![];

    let system = doc.root().children().find(|child| child.tag_name().name() == "system").unwrap();

    // Ensure there is no non-whitespace/comment text
    check_no_text(&xml_sdf, &system)?;

    // Channels cannot be parsed immediately as they refer to a particular protection domain
    // via an index in the list of PDs. This means that we have to parse all PDs first and
    // then parse the channels.
    let mut channel_nodes = Vec::new();

    for child in system.children() {
        if !child.is_element() {
            continue;
        }

        let child_name = child.tag_name().name();
        match child_name {
            "protection_domain" => root_pds.push(ProtectionDomain::from_xml(&xml_sdf, &child, false)?),
            "channel" => channel_nodes.push(child),
            "memory_region" => mrs.push(SysMemoryRegion::from_xml(&xml_sdf, &child, plat_desc)?),
            _ => {
                let pos = xml_sdf.doc.text_pos_at(child.range().start);
                return Err(format!("Invalid XML element '{}': {}", child_name, loc_string(&xml_sdf, pos)))
            }
        }
    }

    let pds = pd_flatten(&xml_sdf, root_pds)?;

    for node in channel_nodes {
        channels.push(Channel::from_xml(&xml_sdf, &node, &pds)?);
    }

    // Now that we have parsed everything in the system description we can validate any
    // global properties (e.g no duplicate PD names etc).

    if pds.is_empty() {
        return Err("Error: at least one protection domain must be defined".to_string());
    }

    if pds.len() > MAX_PDS {
        return Err(format!("Error: too many protection domains ({}) defined. Maximum is {}.", pds.len(), MAX_PDS));
    }

    for pd in &pds {
        if pds.iter().filter(|x| pd.name == x.name).count() > 1 {
            return Err(format!("Error: duplicate protection domain name '{}'.", pd.name));
        }
    }

    for mr in &mrs {
        if mrs.iter().filter(|x| mr.name == x.name).count() > 1 {
            return Err(format!("Error: duplicate memory region name '{}'.", mr.name));
        }
    }

    // Ensure no duplicate IRQs
    let mut all_irqs = Vec::new();
    for pd in &pds {
        for sysirq in &pd.irqs {
            if all_irqs.contains(&sysirq.irq) {
                return Err(format!("Error: duplicate irq: {} in protection domain: '{}' @ {}:{}:{}", sysirq.irq, pd.name, filename, pd.text_pos.row, pd.text_pos.col));
            }
            all_irqs.push(sysirq.irq);
        }
    }

    // Ensure no duplicate channel identifiers.
    // This means checking that no interrupt IDs clash with any channel IDs
    let mut ch_ids = vec![vec![]; pds.len()];
    for (pd_idx, pd) in pds.iter().enumerate() {
        for sysirq in &pd.irqs {
            if ch_ids[pd_idx].contains(&sysirq.id) {
                return Err(format!("Error: duplicate channel id: {} in protection domain: '{}' @ {}:{}:{}", sysirq.id, pd.name, filename, pd.text_pos.row, pd.text_pos.col));
            }
            ch_ids[pd_idx].push(sysirq.id);
        }
    }

    for ch in &channels {
        if ch_ids[ch.pd_a].contains(&ch.id_a) {
            let pd = &pds[ch.pd_a];
            return Err(format!("Error: duplicate channel id: {} in protection domain: '{}' @ {}:{}:{}", ch.id_a, pd.name, filename, pd.text_pos.row, pd.text_pos.col));
        }

        if ch_ids[ch.pd_b].contains(&ch.id_b) {
            let pd = &pds[ch.pd_b];
            return Err(format!("Error: duplicate channel id: {} in protection domain: '{}' @ {}:{}:{}", ch.id_b, pd.name, filename, pd.text_pos.row, pd.text_pos.col));
        }

        ch_ids[ch.pd_a].push(ch.id_a);
        ch_ids[ch.pd_b].push(ch.id_b);
    }

    // Ensure that all maps are correct
    for pd in &pds {
        for map in &pd.maps {
            let maybe_mr = mrs.iter().find(|mr| mr.name == map.mr);
            let pos = map.text_pos.unwrap();
            match maybe_mr {
                Some(mr) => {
                    if map.vaddr % mr.page_size as u64 != 0 {
                        return Err(format!("Error: invalid vaddr alignment on 'map' @ {}", loc_string(&xml_sdf, pos)))
                    }
                },
                None => return Err(format!("Error: invalid memory region name '{}' on 'map' @ {}", map.mr, loc_string(&xml_sdf, pos)))
            };
        }
    }

    Ok(SystemDescription {
        protection_domains: pds,
        memory_regions: mrs,
        channels,
    })
}
