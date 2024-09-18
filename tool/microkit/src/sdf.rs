//
// Copyright 2024, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

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
use crate::sel4::{Config, IrqTrigger, PageSize};
use crate::util::str_to_bool;
use crate::MAX_PDS;
use std::path::{Path, PathBuf};

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
const VCPU_MAX_ID: u64 = PD_MAX_ID;

const PD_MAX_PRIORITY: u8 = 254;
/// In microseconds
const BUDGET_DEFAULT: u64 = 1000;

/// Default to a stack size of a single page
const PD_DEFAULT_STACK_SIZE: u64 = 0x1000;
const PD_MIN_STACK_SIZE: u64 = 0x1000;
const PD_MAX_STACK_SIZE: u64 = 1024 * 1024 * 16;

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
        Err(err) => Err(format!(
            "Error: failed to parse integer '{}' on element '{}': {}",
            s,
            node.tag_name().name(),
            err
        )),
    }
}

fn loc_string(xml_sdf: &XmlSystemDescription, pos: roxmltree::TextPos) -> String {
    format!("{}:{}:{}", xml_sdf.filename, pos.row, pos.col)
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
    pub text_pos: Option<roxmltree::TextPos>,
}

impl SysMemoryRegion {
    /// Given the size of a memory region, returns the 'most optimal'
    /// page size for the platform based on the alignment of the size.
    pub fn optimal_page_size(&self, config: &Config) -> u64 {
        let page_sizes = config.page_sizes();
        for i in (0..page_sizes.len()).rev() {
            if self.size % page_sizes[i] == 0 {
                return page_sizes[i];
            }
        }

        panic!("Internal error: size is not aligned to minimum page size");
    }

    pub fn page_size_bytes(&self) -> u64 {
        self.page_size as u64
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct SysIrq {
    pub irq: u64,
    pub id: u64,
    pub trigger: IrqTrigger,
    pub cpu: u64,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum SysSetVarKind {
    // For size we do not store the size since when we parse mappings
    // we do not have access to the memory region yet. The size is resolved
    // when we actually need to perform the setvar.
    Size { mr: String },
    Vaddr { address: u64 },
    Paddr { region: String },
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct SysSetVar {
    pub symbol: String,
    pub kind: SysSetVarKind,
}

#[derive(Debug, Clone)]
pub struct ChannelEnd {
    pub pd: usize,
    pub id: u64,
    pub notify: bool,
    pub pp: bool,
}

#[derive(Debug)]
pub struct Channel {
    pub end_a: ChannelEnd,
    pub end_b: ChannelEnd,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ProtectionDomain {
    /// Only populated for child protection domains
    pub id: Option<u64>,
    pub name: String,
    pub priority: u8,
    pub budget: u64,
    pub period: u64,
    pub passive: bool,
    pub stack_size: u64,
    pub smc: bool,
    pub cpu: u64,
    pub program_image: PathBuf,
    pub maps: Vec<SysMap>,
    pub irqs: Vec<SysIrq>,
    pub setvars: Vec<SysSetVar>,
    pub virtual_machine: Option<VirtualMachine>,
    /// Only used when parsing child PDs. All elements will be removed
    /// once we flatten each PD and its children into one list.
    pub child_pds: Vec<ProtectionDomain>,
    pub has_children: bool,
    /// Index into the total list of protection domains if a parent
    /// protection domain exists
    pub parent: Option<usize>,
    /// Location in the parsed SDF file
    text_pos: roxmltree::TextPos,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct VirtualMachine {
    pub vcpus: Vec<VirtualCpu>,
    pub name: String,
    pub maps: Vec<SysMap>,
    pub priority: u8,
    pub budget: u64,
    pub period: u64,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct VirtualCpu {
    pub id: u64,
    pub cpu: u64,
}

/// To avoid code duplication for handling protection domains
/// and virtual machines, which have a lot in common.
trait ExecutionContext {
    fn name(&self) -> &String;
    fn kind(&self) -> &'static str;
}

impl ExecutionContext for ProtectionDomain {
    fn name(&self) -> &String {
        &self.name
    }

    fn kind(&self) -> &'static str {
        "protection domain"
    }
}

impl ExecutionContext for VirtualMachine {
    fn name(&self) -> &String {
        &self.name
    }

    fn kind(&self) -> &'static str {
        "virtual machine"
    }
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

impl SysMap {
    fn from_xml(
        xml_sdf: &XmlSystemDescription,
        node: &roxmltree::Node,
        allow_setvar: bool,
        max_vaddr: u64,
    ) -> Result<SysMap, String> {
        let mut attrs = vec!["mr", "vaddr", "perms", "cached"];
        if allow_setvar {
            attrs.push("setvar_vaddr");
            attrs.push("setvar_size");
        }
        check_attributes(xml_sdf, node, &attrs)?;

        let mr = checked_lookup(xml_sdf, node, "mr")?.to_string();
        let vaddr = sdf_parse_number(checked_lookup(xml_sdf, node, "vaddr")?, node)?;

        if vaddr >= max_vaddr {
            return Err(value_error(
                xml_sdf,
                node,
                format!("vaddr (0x{:x}) must be less than 0x{:x}", vaddr, max_vaddr),
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
            text_pos: Some(xml_sdf.doc.text_pos_at(node.range().start)),
        })
    }
}

impl ProtectionDomain {
    pub fn needs_ep(&self, self_id: usize, channels: &[Channel]) -> bool {
        self.has_children
            || self.virtual_machine.is_some()
            || channels.iter().any(|channel| {
                (channel.end_a.pp && channel.end_b.pd == self_id)
                    || (channel.end_b.pp && channel.end_a.pd == self_id)
            })
    }

    fn from_xml(
        config: &Config,
        xml_sdf: &XmlSystemDescription,
        node: &roxmltree::Node,
        is_child: bool,
    ) -> Result<ProtectionDomain, String> {
        let mut attrs = vec![
            "name",
            "priority",
            "budget",
            "period",
            "passive",
            "stack_size",
            "cpu",
            // The 'smc' field is only available in certain configurations
            // but we do the error-checking further down.
            "smc",
        ];
        if is_child {
            attrs.push("id");
        }
        check_attributes(xml_sdf, node, &attrs)?;

        let name = checked_lookup(xml_sdf, node, "name")?.to_string();

        let id = if is_child {
            Some(sdf_parse_number(
                checked_lookup(xml_sdf, node, "id")?,
                node,
            )?)
        } else {
            None
        };

        // If we do not have an explicit budget the period is equal to the default budget.
        let budget = if let Some(xml_budget) = node.attribute("budget") {
            sdf_parse_number(xml_budget, node)?
        } else {
            BUDGET_DEFAULT
        };
        let period = if let Some(xml_period) = node.attribute("period") {
            sdf_parse_number(xml_period, node)?
        } else {
            budget
        };
        if budget > period {
            return Err(value_error(
                xml_sdf,
                node,
                format!(
                    "budget ({}) must be less than, or equal to, period ({})",
                    budget, period
                ),
            ));
        }

        let passive = if let Some(xml_passive) = node.attribute("passive") {
            match str_to_bool(xml_passive) {
                Some(val) => val,
                None => {
                    return Err(value_error(
                        xml_sdf,
                        node,
                        "passive must be 'true' or 'false'".to_string(),
                    ))
                }
            }
        } else {
            false
        };

        let stack_size = if let Some(xml_stack_size) = node.attribute("stack_size") {
            sdf_parse_number(xml_stack_size, node)?
        } else {
            PD_DEFAULT_STACK_SIZE
        };

        let cpu = if let Some(xml_cpu) = node.attribute("cpu") {
            sdf_parse_number(xml_cpu, node)?
        } else {
            // Default to CPU 0, the boot CPU
            0
        };

        if cpu >= config.cores {
            return Err(value_error(xml_sdf, node, format!("cpu given '{}' is invalid, platform is configured with '{}' CPUs", cpu, config.cores)))
        }

        let smc = if let Some(xml_smc) = node.attribute("smc") {
            match str_to_bool(xml_smc) {
                Some(val) => val,
                None => {
                    return Err(value_error(
                        xml_sdf,
                        node,
                        "smc must be 'true' or 'false'".to_string(),
                    ))
                }
            }
        } else {
            false
        };

        if smc {
            match config.arm_smc {
                Some(smc_allowed) => {
                    if !smc_allowed {
                        return Err(value_error(xml_sdf, node, "Using SMC support without ARM SMC forwarding support enabled in the kernel for this platform".to_string()));
                    }
                }
                None => {
                    return Err(
                        "ARM SMC forwarding support is not available for this architecture"
                            .to_string(),
                    )
                }
            }
        }

        #[allow(clippy::manual_range_contains)]
        if stack_size < PD_MIN_STACK_SIZE || stack_size > PD_MAX_STACK_SIZE {
            return Err(value_error(
                xml_sdf,
                node,
                format!(
                    "stack size must be between 0x{:x} bytes and 0x{:x} bytes",
                    PD_MIN_STACK_SIZE, PD_MAX_STACK_SIZE
                ),
            ));
        }

        if stack_size % config.page_sizes()[0] != 0 {
            return Err(value_error(
                xml_sdf,
                node,
                format!(
                    "stack size must be aligned to the smallest page size, {} bytes",
                    config.page_sizes()[0]
                ),
            ));
        }

        let mut maps = Vec::new();
        let mut irqs = Vec::new();
        let mut setvars: Vec<SysSetVar> = Vec::new();
        let mut child_pds = Vec::new();

        let mut program_image = None;
        let mut virtual_machine = None;

        // Default to minimum priority
        let priority = if let Some(xml_priority) = node.attribute("priority") {
            sdf_parse_number(xml_priority, node)?
        } else {
            0
        };

        if priority > PD_MAX_PRIORITY as u64 {
            return Err(value_error(
                xml_sdf,
                node,
                format!("priority must be between 0 and {}", PD_MAX_PRIORITY),
            ));
        }

        for child in node.children() {
            if !child.is_element() {
                continue;
            }

            match child.tag_name().name() {
                "program_image" => {
                    check_attributes(xml_sdf, &child, &["path"])?;
                    if program_image.is_some() {
                        return Err(value_error(
                            xml_sdf,
                            node,
                            "program_image must only be specified once".to_string(),
                        ));
                    }

                    let program_image_path = checked_lookup(xml_sdf, &child, "path")?;
                    program_image = Some(Path::new(program_image_path).to_path_buf());
                }
                "map" => {
                    let map_max_vaddr = config.pd_map_max_vaddr(stack_size);
                    let map = SysMap::from_xml(xml_sdf, &child, true, map_max_vaddr)?;

                    if let Some(setvar_vaddr) = child.attribute("setvar_vaddr") {
                        // Check that the symbol does not already exist
                        for setvar in &setvars {
                            if setvar_vaddr == setvar.symbol {
                                return Err(value_error(
                                    xml_sdf,
                                    &child,
                                    format!("setvar on symbol '{}' already exists", setvar_vaddr),
                                ));
                            }
                        }

                        setvars.push(SysSetVar {
                            symbol: setvar_vaddr.to_string(),
                            kind: SysSetVarKind::Vaddr { address: map.vaddr },
                        });
                    }

                    if let Some(setvar_size) = child.attribute("setvar_size") {
                        // Check that the symbol does not already exist
                        for setvar in &setvars {
                            if setvar_size == setvar.symbol {
                                return Err(value_error(
                                    xml_sdf,
                                    &child,
                                    format!("setvar on symbol '{}' already exists", setvar_size),
                                ));
                            }
                        }

                        setvars.push(SysSetVar {
                            symbol: setvar_size.to_string(),
                            kind: SysSetVarKind::Size { mr: map.mr.clone() },
                        });
                    }

                    maps.push(map);
                }
                "irq" => {
                    check_attributes(xml_sdf, &child, &["irq", "id", "trigger"])?;
                    let irq = checked_lookup(xml_sdf, &child, "irq")?
                        .parse::<u64>()
                        .unwrap();
                    let id = checked_lookup(xml_sdf, &child, "id")?
                        .parse::<i64>()
                        .unwrap();
                    if id > PD_MAX_ID as i64 {
                        return Err(value_error(
                            xml_sdf,
                            &child,
                            format!("id must be < {}", PD_MAX_ID + 1),
                        ));
                    }
                    if id < 0 {
                        return Err(value_error(xml_sdf, &child, "id must be >= 0".to_string()));
                    }

                    let trigger = if let Some(trigger_str) = child.attribute("trigger") {
                        match trigger_str {
                            "level" => IrqTrigger::Level,
                            "edge" => IrqTrigger::Edge,
                            _ => {
                                return Err(value_error(
                                    xml_sdf,
                                    &child,
                                    "trigger must be either 'level' or 'edge'".to_string(),
                                ))
                            }
                        }
                    } else {
                        // Default the level triggered
                        IrqTrigger::Level
                    };

                    let irq = SysIrq {
                        irq,
                        id: id as u64,
                        trigger,
                        cpu,
                    };
                    irqs.push(irq);
                }
                "setvar" => {
                    check_attributes(xml_sdf, &child, &["symbol", "region_paddr"])?;
                    let symbol = checked_lookup(xml_sdf, &child, "symbol")?.to_string();
                    let region = checked_lookup(xml_sdf, &child, "region_paddr")?.to_string();
                    // Check that the symbol does not already exist
                    for setvar in &setvars {
                        if symbol == setvar.symbol {
                            return Err(value_error(
                                xml_sdf,
                                &child,
                                format!("setvar on symbol '{}' already exists", symbol),
                            ));
                        }
                    }
                    setvars.push(SysSetVar {
                        symbol,
                        kind: SysSetVarKind::Paddr { region },
                    })
                }
                "protection_domain" => {
                    child_pds.push(ProtectionDomain::from_xml(config, xml_sdf, &child, true)?)
                }
                "virtual_machine" => {
                    if virtual_machine.is_some() {
                        return Err(value_error(
                            xml_sdf,
                            node,
                            "virtual_machine must only be specified once".to_string(),
                        ));
                    }

                    virtual_machine = Some(VirtualMachine::from_xml(config, xml_sdf, &child)?);
                }
                _ => {
                    let pos = xml_sdf.doc.text_pos_at(child.range().start);
                    return Err(format!(
                        "Invalid XML element '{}': {}",
                        child.tag_name().name(),
                        loc_string(xml_sdf, pos)
                    ));
                }
            }
        }

        if program_image.is_none() {
            return Err(format!(
                "Error: missing 'program_image' element on protection_domain: '{}'",
                name
            ));
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
            passive,
            stack_size,
            smc,
            cpu,
            program_image: program_image.unwrap(),
            maps,
            irqs,
            setvars,
            child_pds,
            virtual_machine,
            has_children,
            parent: None,
            text_pos: xml_sdf.doc.text_pos_at(node.range().start),
        })
    }
}

impl VirtualMachine {
    fn from_xml(
        config: &Config,
        xml_sdf: &XmlSystemDescription,
        node: &roxmltree::Node,
    ) -> Result<VirtualMachine, String> {
        check_attributes(xml_sdf, node, &["name", "budget", "period", "priority"])?;

        let name = checked_lookup(xml_sdf, node, "name")?.to_string();
        // If we do not have an explicit budget the period is equal to the default budget.
        let budget = if let Some(xml_budget) = node.attribute("budget") {
            sdf_parse_number(xml_budget, node)?
        } else {
            BUDGET_DEFAULT
        };
        let period = if let Some(xml_period) = node.attribute("period") {
            sdf_parse_number(xml_period, node)?
        } else {
            budget
        };
        if budget > period {
            return Err(value_error(
                xml_sdf,
                node,
                format!(
                    "budget ({}) must be less than, or equal to, period ({})",
                    budget, period
                ),
            ));
        }

        // Default to minimum priority
        let priority = if let Some(xml_priority) = node.attribute("priority") {
            sdf_parse_number(xml_priority, node)?
        } else {
            0
        };

        let mut vcpus: Vec<VirtualCpu> = Vec::new();
        let mut maps = Vec::new();
        for child in node.children() {
            if !child.is_element() {
                continue;
            }

            let child_name = child.tag_name().name();
            match child_name {
                "vcpu" => {
                    check_attributes(xml_sdf, &child, &["id", "cpu"])?;
                    let id = checked_lookup(xml_sdf, &child, "id")?
                        .parse::<u64>()
                        .unwrap();
                    if id > VCPU_MAX_ID {
                        return Err(value_error(
                            xml_sdf,
                            &child,
                            format!("id must be < {}", VCPU_MAX_ID + 1),
                        ));
                    }

                    for vcpu in &vcpus {
                        if vcpu.id == id {
                            let pos = xml_sdf.doc.text_pos_at(child.range().start);
                            return Err(format!(
                                "Error: duplicate vcpu id {} in virtual machine '{}' @ {}",
                                id,
                                name,
                                loc_string(xml_sdf, pos)
                            ));
                        }
                    }

                    let cpu = if let Some(xml_cpu) = node.attribute("cpu") {
                        sdf_parse_number(xml_cpu, node)?
                    } else {
                        // Default to CPU 0, the boot CPU
                        0
                    };

                    if cpu >= config.cores {
                        return Err(value_error(xml_sdf, node, format!("cpu given '{}' is invalid, platform is configured with '{}' CPUs", cpu, config.cores)))
                    }

                    vcpus.push(VirtualCpu { id, cpu });
                }
                "map" => {
                    // Virtual machines do not have program images and so we do not allow
                    // setvar_vaddr on SysMap
                    let map = SysMap::from_xml(xml_sdf, &child, false, config.vm_map_max_vaddr())?;
                    maps.push(map);
                }
                _ => {
                    let pos = xml_sdf.doc.text_pos_at(node.range().start);
                    return Err(format!(
                        "Error: invalid XML element '{}': {}",
                        child_name,
                        loc_string(xml_sdf, pos)
                    ));
                }
            }
        }

        if vcpus.is_empty() {
            return Err(format!(
                "Error: missing 'vcpu' element on virtual_machine: '{}'",
                name
            ));
        }

        Ok(VirtualMachine {
            vcpus,
            name,
            maps,
            // This downcast is safe as we have checked that this is less than
            // the maximum VM priority, which fits in a u8.
            priority: priority as u8,
            budget,
            period,
        })
    }
}

impl SysMemoryRegion {
    fn from_xml(
        config: &Config,
        xml_sdf: &XmlSystemDescription,
        node: &roxmltree::Node,
    ) -> Result<SysMemoryRegion, String> {
        check_attributes(xml_sdf, node, &["name", "size", "page_size", "phys_addr"])?;

        let name = checked_lookup(xml_sdf, node, "name")?;
        let size = sdf_parse_number(checked_lookup(xml_sdf, node, "size")?, node)?;

        let page_size = if let Some(xml_page_size) = node.attribute("page_size") {
            sdf_parse_number(xml_page_size, node)?
        } else {
            config.page_sizes()[0]
        };

        let page_size_valid = config.page_sizes().contains(&page_size);
        if !page_size_valid {
            return Err(value_error(
                xml_sdf,
                node,
                format!("page size 0x{:x} not supported", page_size),
            ));
        }

        if size % page_size != 0 {
            return Err(value_error(
                xml_sdf,
                node,
                "size is not a multiple of the page size".to_string(),
            ));
        }

        let phys_addr = if let Some(xml_phys_addr) = node.attribute("phys_addr") {
            Some(sdf_parse_number(xml_phys_addr, node)?)
        } else {
            None
        };

        if phys_addr.is_some() && phys_addr.unwrap() % page_size != 0 {
            return Err(value_error(
                xml_sdf,
                node,
                "phys_addr is not aligned to the page size".to_string(),
            ));
        }

        let page_count = size / page_size;

        Ok(SysMemoryRegion {
            name: name.to_string(),
            size,
            page_size: page_size.into(),
            page_count,
            phys_addr,
            text_pos: Some(xml_sdf.doc.text_pos_at(node.range().start)),
        })
    }
}

impl ChannelEnd {
    fn from_xml<'a>(
        xml_sdf: &'a XmlSystemDescription,
        node: &'a roxmltree::Node,
        pds: &[ProtectionDomain],
    ) -> Result<ChannelEnd, String> {
        let node_name = node.tag_name().name();
        if node_name != "end" {
            let pos = xml_sdf.doc.text_pos_at(node.range().start);
            return Err(format!(
                "Error: invalid XML element '{}': {}",
                node_name,
                loc_string(xml_sdf, pos)
            ));
        }

        check_attributes(xml_sdf, node, &["pd", "id", "pp", "notify"])?;
        let end_pd = checked_lookup(xml_sdf, node, "pd")?;
        let end_id = checked_lookup(xml_sdf, node, "id")?.parse::<i64>().unwrap();

        if end_id > PD_MAX_ID as i64 {
            return Err(value_error(
                xml_sdf,
                node,
                format!("id must be < {}", PD_MAX_ID + 1),
            ));
        }

        if end_id < 0 {
            return Err(value_error(xml_sdf, node, "id must be >= 0".to_string()));
        }

        let notify = node
            .attribute("notify")
            .map(str_to_bool)
            .unwrap_or(Some(true))
            .ok_or_else(|| {
                value_error(
                    xml_sdf,
                    node,
                    "notify must be 'true' or 'false'".to_string(),
                )
            })?;

        let pp = node
            .attribute("pp")
            .map(str_to_bool)
            .unwrap_or(Some(false))
            .ok_or_else(|| {
                value_error(xml_sdf, node, "pp must be 'true' or 'false'".to_string())
            })?;

        if let Some(pd_idx) = pds.iter().position(|pd| pd.name == end_pd) {
            Ok(ChannelEnd {
                pd: pd_idx,
                id: end_id.try_into().unwrap(),
                notify,
                pp,
            })
        } else {
            Err(value_error(
                xml_sdf,
                node,
                format!("invalid PD name '{end_pd}'"),
            ))
        }
    }
}

impl Channel {
    /// It should be noted that this function assumes that `pds` is populated
    /// with all the Protection Domains that could potentially be connected with
    /// the channel.
    fn from_xml<'a>(
        xml_sdf: &'a XmlSystemDescription,
        node: &'a roxmltree::Node,
        pds: &[ProtectionDomain],
    ) -> Result<Channel, String> {
        check_attributes(xml_sdf, node, &[])?;

        let [ref end_a, ref end_b] = node
            .children()
            .filter(|child| child.is_element())
            .map(|node| ChannelEnd::from_xml(xml_sdf, &node, pds))
            .collect::<Result<Vec<_>, _>>()?[..]
        else {
            return Err(value_error(
                xml_sdf,
                node,
                "exactly two end elements must be specified".to_string(),
            ));
        };

        if end_a.pp && end_b.pp {
            return Err(value_error(
                xml_sdf,
                node,
                "cannot ppc bidirectionally".to_string(),
            ));
        }

        Ok(Channel {
            end_a: end_a.clone(),
            end_b: end_b.clone(),
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

fn check_maps(
    xml_sdf: &XmlSystemDescription,
    mrs: &[SysMemoryRegion],
    e: &dyn ExecutionContext,
    maps: &[SysMap],
) -> Result<(), String> {
    let mut checked_maps = Vec::with_capacity(maps.len());
    for map in maps {
        let maybe_mr = mrs.iter().find(|mr| mr.name == map.mr);
        let pos = map.text_pos.unwrap();
        match maybe_mr {
            Some(mr) => {
                if map.vaddr % mr.page_size_bytes() != 0 {
                    return Err(format!(
                        "Error: invalid vaddr alignment on 'map' @ {}",
                        loc_string(xml_sdf, pos)
                    ));
                }

                let map_start = map.vaddr;
                let map_end = map.vaddr + mr.size;
                for (name, start, end) in &checked_maps {
                    if !(map_start >= *end || map_end <= *start) {
                        return Err(
                            format!(
                                "Error: map for '{}' has virtual address range [0x{:x}..0x{:x}) which overlaps with map for '{}' [0x{:x}..0x{:x}) in {} '{}' @ {}",
                                map.mr,
                                map_start,
                                map_end,
                                name,
                                start,
                                end,
                                e.kind(),
                                e.name(),
                                loc_string(xml_sdf, map.text_pos.unwrap())
                            )
                        );
                    }
                }
                checked_maps.push((&map.mr, map_start, map_end));
            }
            None => {
                return Err(format!(
                    "Error: invalid memory region name '{}' on 'map' @ {}",
                    map.mr,
                    loc_string(xml_sdf, pos)
                ))
            }
        };
    }

    Ok(())
}

fn check_attributes(
    xml_sdf: &XmlSystemDescription,
    node: &roxmltree::Node,
    attributes: &[&'static str],
) -> Result<(), String> {
    for attribute in node.attributes() {
        if !attributes.contains(&attribute.name()) {
            return Err(value_error(
                xml_sdf,
                node,
                format!("invalid attribute '{}'", attribute.name()),
            ));
        }
    }

    Ok(())
}

fn checked_lookup<'a>(
    xml_sdf: &XmlSystemDescription,
    node: &'a roxmltree::Node,
    attribute: &'static str,
) -> Result<&'a str, String> {
    if let Some(value) = node.attribute(attribute) {
        Ok(value)
    } else {
        let pos = xml_sdf.doc.text_pos_at(node.range().start);
        Err(format!(
            "Error: Missing required attribute '{}' on element '{}': {}:{}:{}",
            attribute,
            node.tag_name().name(),
            xml_sdf.filename,
            pos.row,
            pos.col
        ))
    }
}

fn value_error(xml_sdf: &XmlSystemDescription, node: &roxmltree::Node, err: String) -> String {
    let pos = xml_sdf.doc.text_pos_at(node.range().start);
    format!(
        "Error: {} on element '{}': {}:{}:{}",
        err,
        node.tag_name().name(),
        xml_sdf.filename,
        pos.row,
        pos.col
    )
}

fn check_no_text(xml_sdf: &XmlSystemDescription, node: &roxmltree::Node) -> Result<(), String> {
    let name = node.tag_name().name();
    let pos = xml_sdf.doc.text_pos_at(node.range().start);

    if let Some(text) = node.text() {
        // If the text is just whitespace then it is okay
        if !text.trim().is_empty() {
            return Err(format!(
                "Error: unexpected text found in element '{}' @ {}",
                name,
                loc_string(xml_sdf, pos)
            ));
        }
    }

    if node.tail().is_some() {
        return Err(format!(
            "Error: unexpected text found after element '{}' @ {}",
            name,
            loc_string(xml_sdf, pos)
        ));
    }

    for child in node.children() {
        if !child.is_comment() && !child.is_element() {
            check_no_text(xml_sdf, &child)?;
        }
    }

    Ok(())
}

/// Take a PD and return a vector with the given PD at the start and all of the children PDs following.
///
/// For example if PD A had children B, C then we would have [A, B, C].
/// If we had the same example but child B also had a child D, we would have [A, B, D, C].
fn pd_tree_to_list(
    xml_sdf: &XmlSystemDescription,
    mut pd: ProtectionDomain,
    idx: usize,
) -> Result<Vec<ProtectionDomain>, String> {
    let mut child_ids = vec![];
    for child_pd in &pd.child_pds {
        let child_id = child_pd.id.unwrap();
        if child_ids.contains(&child_id) {
            return Err(format!(
                "Error: duplicate id: {} in protection domain: '{}' @ {}",
                child_id,
                pd.name,
                loc_string(xml_sdf, child_pd.text_pos)
            ));
        }
        // Also check that the child ID does not clash with any vCPU IDs, if the PD has a virtual machine
        if let Some(vm) = &pd.virtual_machine {
            for vcpu in &vm.vcpus {
                if child_id == vcpu.id {
                    return Err(format!("Error: duplicate id: {} clashes with virtual machine vcpu id in protection domain: '{}' @ {}",
                                        child_id, pd.name, loc_string(xml_sdf, child_pd.text_pos)));
                }
            }
        }
        child_ids.push(child_id);
    }

    let mut new_child_pds = vec![];
    let child_pds: Vec<_> = pd.child_pds.drain(0..).collect();
    for mut child_pd in child_pds {
        // The parent PD's index is set for each child. We then pass the index relative to the *total*
        // list to any nested children so their parent index can be set to the position of this child.
        child_pd.parent = Some(idx);
        new_child_pds.extend(pd_tree_to_list(
            xml_sdf,
            child_pd,
            // We need to pass the position of this current child PD in the global list.
            // `idx` is this child's parent index in the global list, so we need to add
            // the position of this child to `idx` which will be the number of extra child
            // PDs we've just processed, plus one for the actual entry of this child.
            idx + new_child_pds.len() + 1,
        )?);
    }

    let mut all = vec![pd];
    all.extend(new_child_pds);

    Ok(all)
}

/// Given an iterable of protection domains flatten the tree representation
/// into a flat tuple.
///
/// In doing so the representation is changed from "Node with list of children",
/// to each node having a parent link instead.
fn pd_flatten(
    xml_sdf: &XmlSystemDescription,
    pds: Vec<ProtectionDomain>,
) -> Result<Vec<ProtectionDomain>, String> {
    let mut all_pds = vec![];

    for pd in pds {
        // These are all root PDs, so should not have parents.
        assert!(pd.parent.is_none());
        // We provide the index of the PD in the entire PD list
        all_pds.extend(pd_tree_to_list(xml_sdf, pd, all_pds.len())?);
    }

    Ok(all_pds)
}

pub fn parse(filename: &str, xml: &str, config: &Config) -> Result<SystemDescription, String> {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(doc) => doc,
        Err(err) => return Err(format!("Could not parse '{}': {}", filename, err)),
    };

    let xml_sdf = XmlSystemDescription {
        filename,
        doc: &doc,
    };

    let mut root_pds = vec![];
    let mut mrs = vec![];
    let mut channels = vec![];

    let system = doc
        .root()
        .children()
        .find(|child| child.tag_name().name() == "system")
        .unwrap();

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
            "protection_domain" => {
                root_pds.push(ProtectionDomain::from_xml(config, &xml_sdf, &child, false)?)
            }
            "channel" => channel_nodes.push(child),
            "memory_region" => mrs.push(SysMemoryRegion::from_xml(config, &xml_sdf, &child)?),
            "virtual_machine" => {
                let pos = xml_sdf.doc.text_pos_at(child.range().start);
                return Err(format!(
                    "Error: virtual machine must be a child of a protection domain: {}",
                    loc_string(&xml_sdf, pos)
                ));
            }
            _ => {
                let pos = xml_sdf.doc.text_pos_at(child.range().start);
                return Err(format!(
                    "Error: invalid XML element '{}': {}",
                    child_name,
                    loc_string(&xml_sdf, pos)
                ));
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
        return Err(format!(
            "Error: too many protection domains ({}) defined. Maximum is {}.",
            pds.len(),
            MAX_PDS
        ));
    }

    for pd in &pds {
        if pds.iter().filter(|x| pd.name == x.name).count() > 1 {
            return Err(format!(
                "Error: duplicate protection domain name '{}'.",
                pd.name
            ));
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

    let mut vms = vec![];
    for pd in &pds {
        if let Some(vm) = &pd.virtual_machine {
            if vms.contains(&vm) {
                return Err(format!(
                    "Error: duplicate virtual machine name '{}'.",
                    vm.name
                ));
            }
            vms.push(vm);
        }
    }

    // Ensure no duplicate IRQs
    let mut all_irqs = Vec::new();
    for pd in &pds {
        for sysirq in &pd.irqs {
            if all_irqs.contains(&sysirq.irq) {
                return Err(format!(
                    "Error: duplicate irq: {} in protection domain: '{}' @ {}:{}:{}",
                    sysirq.irq, pd.name, filename, pd.text_pos.row, pd.text_pos.col
                ));
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
                return Err(format!(
                    "Error: duplicate channel id: {} in protection domain: '{}' @ {}:{}:{}",
                    sysirq.id, pd.name, filename, pd.text_pos.row, pd.text_pos.col
                ));
            }
            ch_ids[pd_idx].push(sysirq.id);
        }
    }

    for ch in &channels {
        if ch_ids[ch.end_a.pd].contains(&ch.end_a.id) {
            let pd = &pds[ch.end_a.pd];
            return Err(format!(
                "Error: duplicate channel id: {} in protection domain: '{}' @ {}:{}:{}",
                ch.end_a.id, pd.name, filename, pd.text_pos.row, pd.text_pos.col
            ));
        }

        if ch_ids[ch.end_b.pd].contains(&ch.end_b.id) {
            let pd = &pds[ch.end_b.pd];
            return Err(format!(
                "Error: duplicate channel id: {} in protection domain: '{}' @ {}:{}:{}",
                ch.end_b.id, pd.name, filename, pd.text_pos.row, pd.text_pos.col
            ));
        }

        let pd_a = &pds[ch.end_a.pd];
        let pd_b = &pds[ch.end_b.pd];
        if ch.end_a.pp && pd_a.priority >= pd_b.priority {
            return Err(format!(
                "Error: PPCs must be to protection domains of strictly higher priorities; \
                        channel with PPC exists from pd {} (priority: {}) to pd {} (priority: {})",
                pd_a.name, pd_a.priority, pd_b.name, pd_b.priority
            ));
        } else if ch.end_b.pp && pd_b.priority >= pd_a.priority {
            return Err(format!(
                "Error: PPCs must be to protection domains of strictly higher priorities; \
                        channel with PPC exists from pd {} (priority: {}) to pd {} (priority: {})",
                pd_b.name, pd_b.priority, pd_a.name, pd_a.priority
            ));
        }

        ch_ids[ch.end_a.pd].push(ch.end_a.id);
        ch_ids[ch.end_b.pd].push(ch.end_b.id);
    }

    // Ensure that all maps are correct
    for pd in &pds {
        check_maps(&xml_sdf, &mrs, pd, &pd.maps)?;
        if let Some(vm) = &pd.virtual_machine {
            check_maps(&xml_sdf, &mrs, vm, &vm.maps)?;
        }
    }

    // Ensure MRs with physical addresses do not overlap
    let mut checked_mrs = Vec::with_capacity(mrs.len());
    for mr in &mrs {
        if let Some(phys_addr) = mr.phys_addr {
            let mr_start = phys_addr;
            let mr_end = phys_addr + mr.size;

            for (name, start, end) in &checked_mrs {
                if !(mr_start >= *end || mr_end <= *start) {
                    let pos = mr.text_pos.unwrap();
                    return Err(
                        format!(
                            "Error: memory region '{}' physical address range [0x{:x}..0x{:x}) overlaps with another memory region '{}' [0x{:x}..0x{:x}) @ {}",
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
    for pd in &pds {
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

    // Optimise page size of MRs, if we can
    for mr in &mut mrs {
        // If the largest possible page size based on the MR's size is already
        // set as its page size, skip it.
        let mr_larget_page_size = mr.optimal_page_size(config);
        if mr.page_size_bytes() == mr_larget_page_size {
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
        if let Some(paddr) = mr.phys_addr {
            addrs.push(paddr);
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
            if addrs.iter().any(|addr| addr % larger_page_size != 0) {
                continue;
            }

            // Safe to increase page size
            mr.page_size = larger_page_size.into();
            mr.page_count = mr.size / mr.page_size_bytes();
        }
    }

    Ok(SystemDescription {
        protection_domains: pds,
        memory_regions: mrs,
        channels,
    })
}
