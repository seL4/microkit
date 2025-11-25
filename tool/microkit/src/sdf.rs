//
// Copyright 2025, UNSW
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
use crate::sel4::{
    Arch, ArmRiscvIrqTrigger, Config, PageSize, X86IoapicIrqPolarity, X86IoapicIrqTrigger,
};
use crate::util::{ranges_overlap, str_to_bool};
use crate::MAX_PDS;
use std::collections::HashSet;
use std::fmt::Display;
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

pub const MONITOR_PRIORITY: u8 = 255;
const PD_MAX_PRIORITY: u8 = 254;
/// In microseconds
pub const BUDGET_DEFAULT: u64 = 1000;

pub const MONITOR_PD_NAME: &str = "monitor";

/// Default to a stack size of 8KiB
pub const PD_DEFAULT_STACK_SIZE: u64 = 0x2000;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysMap {
    pub mr: String,
    pub vaddr: u64,
    pub perms: u8,
    pub cached: bool,
    /// Location in the parsed SDF file. Because this struct is
    /// used in a non-XML context, we make the position optional.
    pub text_pos: Option<roxmltree::TextPos>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum SysMemoryRegionKind {
    User,
    Elf,
    Stack,
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
    page_size_specified_by_user: bool,
    pub page_size: PageSize,
    pub page_count: u64,
    pub phys_addr: SysMemoryRegionPaddr,
    pub text_pos: Option<roxmltree::TextPos>,
    /// For error reporting is useful to know whether the MR was created
    /// due to the user's SDF or created by the tool for setting up the
    /// stack, ELF, etc.
    pub kind: SysMemoryRegionKind,
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

#[derive(Debug, PartialEq, Eq)]
pub enum SysIrqKind {
    Conventional {
        irq: u64,
        trigger: ArmRiscvIrqTrigger,
    },
    /// x86-64 specific
    IOAPIC {
        ioapic: u64,
        pin: u64,
        trigger: X86IoapicIrqTrigger,
        polarity: X86IoapicIrqPolarity,
        vector: u64,
    },
    /// x86-64 specific
    MSI {
        pci_bus: u64,
        pci_dev: u64,
        pci_func: u64,
        handle: u64,
        vector: u64,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct SysIrq {
    pub id: u64,
    pub kind: SysIrqKind,
}

impl SysIrq {
    pub fn irq_num(&self) -> u64 {
        match self.kind {
            SysIrqKind::Conventional { irq, .. } => irq,
            SysIrqKind::IOAPIC { vector, .. } => vector,
            SysIrqKind::MSI { vector, .. } => vector,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct IOPort {
    pub id: u64,
    pub addr: u64,
    pub size: u64,
    pub text_pos: roxmltree::TextPos,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SysSetVarKind {
    // For size we do not store the size since when we parse mappings
    // we do not have access to the memory region yet. The size is resolved
    // when we actually need to perform the setvar.
    Size { mr: String },
    Vaddr { address: u64 },
    Paddr { region: String },
    Id { id: u64 },
    X86IoPortAddr { address: u64 },
}

#[derive(Debug, PartialEq, Eq)]
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
    pub setvar_id: Option<String>,
}

#[derive(Debug)]
pub struct Channel {
    pub end_a: ChannelEnd,
    pub end_b: ChannelEnd,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CpuCore(pub u8);

impl Display for CpuCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("cpu{:02}", self.0))
    }
}

#[derive(Debug, PartialEq, Eq)]
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
    pub cpu: CpuCore,
    pub program_image: PathBuf,
    pub maps: Vec<SysMap>,
    pub irqs: Vec<SysIrq>,
    pub ioports: Vec<IOPort>,
    pub setvars: Vec<SysSetVar>,
    pub virtual_machine: Option<VirtualMachine>,
    /// Only used when parsing child PDs. All elements will be removed
    /// once we flatten each PD and its children into one list.
    pub child_pds: Vec<ProtectionDomain>,
    pub has_children: bool,
    /// Index into the total list of protection domains if a parent
    /// protection domain exists
    pub parent: Option<usize>,
    /// Value of the setvar_id attribute, if a parent protection domain exists
    pub setvar_id: Option<String>,
    /// Location in the parsed SDF file
    text_pos: Option<roxmltree::TextPos>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct VirtualMachine {
    pub vcpus: Vec<VirtualCpu>,
    pub name: String,
    pub maps: Vec<SysMap>,
    pub priority: u8,
    pub budget: u64,
    pub period: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct VirtualCpu {
    pub id: u64,
    pub setvar_id: Option<String>,
    pub cpu: CpuCore,
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
                format!("vaddr (0x{vaddr:x}) must be less than 0x{max_vaddr:x}"),
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

    pub fn irq_bits(&self) -> u64 {
        let mut irqs = 0;
        for irq in &self.irqs {
            irqs |= 1 << irq.id;
        }

        irqs
    }

    pub fn ioport_bits(&self) -> u64 {
        let mut ioports = 0;
        for ioport in &self.ioports {
            ioports |= 1 << ioport.id;
        }

        ioports
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
            // The SMC field is only available in certain configurations
            // but we do the error-checking further down.
            "smc",
            "cpu",
        ];
        if is_child {
            attrs.push("id");
            attrs.push("setvar_id");
        }
        check_attributes(xml_sdf, node, &attrs)?;

        let name = checked_lookup(xml_sdf, node, "name")?.to_string();

        let (id, setvar_id) = if is_child {
            let id = sdf_parse_number(checked_lookup(xml_sdf, node, "id")?, node)?;
            let setvar_id = node.attribute("setvar_id").map(ToOwned::to_owned);
            (Some(id), setvar_id)
        } else {
            (None, None)
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
                format!("budget ({budget}) must be less than, or equal to, period ({period})"),
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

        let cpu = CpuCore(
            sdf_parse_number(node.attribute("cpu").unwrap_or("0"), node)?
                .try_into()
                .expect("cpu core must be between 0 and 255"),
        );

        if cpu.0 >= config.num_cores {
            return Err(value_error(
                xml_sdf,
                node,
                format!(
                    "cpu core must be less than {}, got {}",
                    config.num_cores, cpu
                ),
            ));
        }

        #[allow(clippy::manual_range_contains)]
        if stack_size < PD_MIN_STACK_SIZE || stack_size > PD_MAX_STACK_SIZE {
            return Err(value_error(
                xml_sdf,
                node,
                format!(
                    "stack size must be between 0x{PD_MIN_STACK_SIZE:x} bytes and 0x{PD_MAX_STACK_SIZE:x} bytes"
                ),
            ));
        }

        if !stack_size.is_multiple_of(config.page_sizes()[0]) {
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
        let mut ioports = Vec::new();
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
                format!("priority must be between 0 and {PD_MAX_PRIORITY}"),
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
                        let setvar = SysSetVar {
                            symbol: setvar_vaddr.to_string(),
                            kind: SysSetVarKind::Vaddr { address: map.vaddr },
                        };
                        checked_add_setvar(&mut setvars, setvar, xml_sdf, &child)?;
                    }

                    if let Some(setvar_size) = child.attribute("setvar_size") {
                        let setvar = SysSetVar {
                            symbol: setvar_size.to_string(),
                            kind: SysSetVarKind::Size { mr: map.mr.clone() },
                        };
                        checked_add_setvar(&mut setvars, setvar, xml_sdf, &child)?;
                    }

                    maps.push(map);
                }
                "irq" => {
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

                    if let Some(setvar_id) = child.attribute("setvar_id") {
                        let setvar = SysSetVar {
                            symbol: setvar_id.to_string(),
                            kind: SysSetVarKind::Id { id: id as u64 },
                        };
                        checked_add_setvar(&mut setvars, setvar, xml_sdf, &child)?;
                    }

                    if let Some(irq_str) = child.attribute("irq") {
                        if config.arch == Arch::X86_64 {
                            return Err(value_error(
                                xml_sdf,
                                &child,
                                "ARM and RISC-V IRQs are not supported on x86".to_string(),
                            ));
                        }

                        // ARM and RISC-V interrupts must have an "irq" attribute.
                        check_attributes(xml_sdf, &child, &["irq", "id", "setvar_id", "trigger"])?;
                        let irq = irq_str.parse::<u64>().unwrap();
                        let trigger = if let Some(trigger_str) = child.attribute("trigger") {
                            match trigger_str {
                                "level" => ArmRiscvIrqTrigger::Level,
                                "edge" => ArmRiscvIrqTrigger::Edge,
                                _ => {
                                    return Err(value_error(
                                        xml_sdf,
                                        &child,
                                        "trigger must be either 'level' or 'edge'".to_string(),
                                    ))
                                }
                            }
                        } else {
                            // Default to level triggered
                            ArmRiscvIrqTrigger::Level
                        };
                        let irq = SysIrq {
                            id: id as u64,
                            kind: SysIrqKind::Conventional { irq, trigger },
                        };
                        irqs.push(irq);
                    } else if let Some(pin_str) = child.attribute("pin") {
                        if config.arch != Arch::X86_64 {
                            return Err(value_error(
                                xml_sdf,
                                &child,
                                "x86 I/O APIC IRQ isn't supported on ARM and RISC-V".to_string(),
                            ));
                        }

                        // IOAPIC interrupts (X86_64) must have a "pin" attribute.
                        check_attributes(
                            xml_sdf,
                            &child,
                            &[
                                "id",
                                "setvar_id",
                                "ioapic",
                                "pin",
                                "trigger",
                                "polarity",
                                "vector",
                            ],
                        )?;

                        let ioapic = if let Some(ioapic_str) = child.attribute("ioapic") {
                            ioapic_str.parse::<i64>().unwrap()
                        } else {
                            // Default to the first unit.
                            0
                        };
                        if ioapic < 0 {
                            return Err(value_error(
                                xml_sdf,
                                &child,
                                "ioapic must be >= 0".to_string(),
                            ));
                        }

                        let pin = pin_str.parse::<i64>().unwrap();
                        if pin < 0 {
                            return Err(value_error(
                                xml_sdf,
                                &child,
                                "pin must be >= 0".to_string(),
                            ));
                        }

                        let trigger = if let Some(trigger_str) = child.attribute("trigger") {
                            match trigger_str {
                                "level" => X86IoapicIrqTrigger::Level,
                                "edge" => X86IoapicIrqTrigger::Edge,
                                _ => {
                                    return Err(value_error(
                                        xml_sdf,
                                        &child,
                                        "trigger must be either 'level' or 'edge'".to_string(),
                                    ))
                                }
                            }
                        } else {
                            // Default to level trigger.
                            X86IoapicIrqTrigger::Level
                        };
                        let polarity = if let Some(polarity_str) = child.attribute("polarity") {
                            match polarity_str {
                                "low" => X86IoapicIrqPolarity::LowTriggered,
                                "high" => X86IoapicIrqPolarity::HighTriggered,
                                _ => {
                                    return Err(value_error(
                                        xml_sdf,
                                        &child,
                                        "polarity must be either 'low' or 'high'".to_string(),
                                    ))
                                }
                            }
                        } else {
                            // Default to normal polarity
                            X86IoapicIrqPolarity::HighTriggered
                        };
                        let vector = checked_lookup(xml_sdf, &child, "vector")?
                            .parse::<i64>()
                            .unwrap();
                        if vector < 0 {
                            return Err(value_error(
                                xml_sdf,
                                &child,
                                "vector must be >= 0".to_string(),
                            ));
                        }

                        let irq = SysIrq {
                            id: id as u64,
                            kind: SysIrqKind::IOAPIC {
                                ioapic: ioapic as u64,
                                pin: pin as u64,
                                trigger,
                                polarity,
                                vector: vector as u64,
                            },
                        };
                        irqs.push(irq);
                    } else if let Some(pcidev_str) = child.attribute("pcidev") {
                        if config.arch != Arch::X86_64 {
                            return Err(value_error(
                                xml_sdf,
                                &child,
                                "x86 MSI IRQ isn't supported on ARM and RISC-V".to_string(),
                            ));
                        }

                        // MSI interrupts (X86_64) have a "pcidev" attribute.
                        check_attributes(
                            xml_sdf,
                            &child,
                            &["id", "setvar_id", "pcidev", "handle", "vector"],
                        )?;

                        let pci_parts: Vec<i64> = pcidev_str
                            .split([':', '.'])
                            .map(str::trim)
                            .map(|x| {
                                i64::from_str_radix(x, 16).expect(
                                    "Error: Failed to parse parts of the PCI device address",
                                )
                            })
                            .collect();
                        if pci_parts.len() != 3 {
                            return Err(format!(
                                "Error: failed to parse PCI address '{}' on element '{}'",
                                pcidev_str,
                                child.tag_name().name()
                            ));
                        }
                        if pci_parts[0] < 0 {
                            return Err(value_error(
                                xml_sdf,
                                &child,
                                "PCI bus must be >= 0".to_string(),
                            ));
                        }
                        if pci_parts[1] < 0 {
                            return Err(value_error(
                                xml_sdf,
                                &child,
                                "PCI device must be >= 0".to_string(),
                            ));
                        }
                        if pci_parts[2] < 0 {
                            return Err(value_error(
                                xml_sdf,
                                &child,
                                "PCI function must be >= 0".to_string(),
                            ));
                        }

                        let handle = checked_lookup(xml_sdf, &child, "handle")?
                            .parse::<i64>()
                            .unwrap();
                        if handle < 0 {
                            return Err(value_error(
                                xml_sdf,
                                &child,
                                "handle must be >= 0".to_string(),
                            ));
                        }

                        let vector = checked_lookup(xml_sdf, &child, "vector")?
                            .parse::<i64>()
                            .unwrap();
                        if vector < 0 {
                            return Err(value_error(
                                xml_sdf,
                                &child,
                                "vector must be >= 0".to_string(),
                            ));
                        }

                        let irq = SysIrq {
                            id: id as u64,
                            kind: SysIrqKind::MSI {
                                pci_bus: pci_parts[0] as u64,
                                pci_dev: pci_parts[1] as u64,
                                pci_func: pci_parts[2] as u64,
                                handle: handle as u64,
                                vector: vector as u64,
                            },
                        };
                        irqs.push(irq);
                    } else {
                        // We can't figure out what type interrupt is specified.
                        // Trigger an error.
                        match config.arch {
                            Arch::Aarch64 | Arch::Riscv64 => {
                                checked_lookup(xml_sdf, &child, "irq")?
                            }
                            Arch::X86_64 => {
                                checked_lookup(xml_sdf, &child, "pin")?;
                                checked_lookup(xml_sdf, &child, "pcidev")?
                            }
                        };
                    }
                }
                "ioport" => {
                    if let Arch::X86_64 = config.arch {
                        check_attributes(
                            xml_sdf,
                            &child,
                            &["id", "setvar_id", "setvar_addr", "addr", "size"],
                        )?;

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
                            return Err(value_error(
                                xml_sdf,
                                &child,
                                "id must be >= 0".to_string(),
                            ));
                        }

                        if let Some(setvar_id) = child.attribute("setvar_id") {
                            let setvar = SysSetVar {
                                symbol: setvar_id.to_string(),
                                kind: SysSetVarKind::Id { id: id as u64 },
                            };
                            checked_add_setvar(&mut setvars, setvar, xml_sdf, &child)?;
                        }

                        let addr =
                            sdf_parse_number(checked_lookup(xml_sdf, &child, "addr")?, &child)?;

                        if let Some(setvar_addr) = child.attribute("setvar_addr") {
                            let setvar = SysSetVar {
                                symbol: setvar_addr.to_string(),
                                kind: SysSetVarKind::X86IoPortAddr { address: addr },
                            };
                            checked_add_setvar(&mut setvars, setvar, xml_sdf, &child)?;
                        }

                        let size = checked_lookup(xml_sdf, &child, "size")?
                            .parse::<i64>()
                            .unwrap();
                        if size <= 0 {
                            return Err(value_error(
                                xml_sdf,
                                &child,
                                "size must be > 0".to_string(),
                            ));
                        }

                        ioports.push(IOPort {
                            id: id as u64,
                            addr,
                            size: size as u64,
                            text_pos: xml_sdf.doc.text_pos_at(node.range().start),
                        })
                    } else {
                        return Err(value_error(
                            xml_sdf,
                            node,
                            "I/O Ports are only available on x86".to_string(),
                        ));
                    }
                }
                "setvar" => {
                    match config.arch {
                        Arch::Aarch64 | Arch::Riscv64 => {}
                        Arch::X86_64 => {
                            return Err(value_error(
                                xml_sdf,
                                node,
                                "setvar with 'region_paddr' for MR without a specified paddr is unsupported on x86_64".to_string(),
                            ));
                        }
                    };

                    check_attributes(xml_sdf, &child, &["symbol", "region_paddr"])?;
                    let symbol = checked_lookup(xml_sdf, &child, "symbol")?.to_string();
                    let region = checked_lookup(xml_sdf, &child, "region_paddr")?.to_string();
                    let setvar = SysSetVar {
                        symbol,
                        kind: SysSetVarKind::Paddr { region },
                    };
                    checked_add_setvar(&mut setvars, setvar, xml_sdf, &child)?;
                }
                "protection_domain" => {
                    let child_pd = ProtectionDomain::from_xml(config, xml_sdf, &child, true)?;

                    if let Some(setvar_id) = &child_pd.setvar_id {
                        let setvar = SysSetVar {
                            symbol: setvar_id.to_string(),
                            kind: SysSetVarKind::Id {
                                id: child_pd.id.unwrap(),
                            },
                        };
                        checked_add_setvar(&mut setvars, setvar, xml_sdf, &child)?;
                    }

                    child_pds.push(child_pd);
                }
                "virtual_machine" => {
                    if !config.hypervisor {
                        return Err(value_error(
                            xml_sdf,
                            node,
                            "seL4 has not been built as a hypervisor, virtual machiens are disabled".to_string()
                        ));
                    }
                    if virtual_machine.is_some() {
                        return Err(value_error(
                            xml_sdf,
                            node,
                            "virtual_machine must only be specified once".to_string(),
                        ));
                    }

                    let vm = VirtualMachine::from_xml(config, xml_sdf, &child)?;

                    for vcpu in &vm.vcpus {
                        if let Some(setvar_id) = &vcpu.setvar_id {
                            let setvar = SysSetVar {
                                symbol: setvar_id.to_string(),
                                kind: SysSetVarKind::Id { id: vcpu.id },
                            };
                            checked_add_setvar(&mut setvars, setvar, xml_sdf, &child)?;
                        }
                    }

                    virtual_machine = Some(vm);
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
                "Error: missing 'program_image' element on protection_domain: '{name}'"
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
            ioports,
            setvars,
            child_pds,
            virtual_machine,
            has_children,
            parent: None,
            setvar_id,
            text_pos: Some(xml_sdf.doc.text_pos_at(node.range().start)),
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
                format!("budget ({budget}) must be less than, or equal to, period ({period})"),
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
                    check_attributes(xml_sdf, &child, &["id", "setvar_id", "cpu"])?;
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

                    let setvar_id = node.attribute("setvar_id").map(ToOwned::to_owned);

                    let cpu = CpuCore(
                        sdf_parse_number(child.attribute("cpu").unwrap_or("0"), node)?
                            .try_into()
                            .expect("cpu # fits in u8"),
                    );

                    if cpu.0 >= config.num_cores {
                        return Err(value_error(
                            xml_sdf,
                            &child,
                            format!(
                                "cpu core must be less than {}, got {}",
                                config.num_cores, cpu
                            ),
                        ));
                    }

                    vcpus.push(VirtualCpu { id, setvar_id, cpu });
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
                "Error: missing 'vcpu' element on virtual_machine: '{name}'"
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
                format!("page size 0x{page_size:x} not supported"),
            ));
        }

        if !size.is_multiple_of(page_size) {
            return Err(value_error(
                xml_sdf,
                node,
                "size is not a multiple of the page size".to_string(),
            ));
        }

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
            text_pos: Some(xml_sdf.doc.text_pos_at(node.range().start)),
            kind: SysMemoryRegionKind::User,
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

        check_attributes(xml_sdf, node, &["pd", "id", "pp", "notify", "setvar_id"])?;
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
            let setvar_id = node.attribute("setvar_id").map(ToOwned::to_owned);
            Ok(ChannelEnd {
                pd: pd_idx,
                id: end_id.try_into().unwrap(),
                notify,
                pp,
                setvar_id,
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
                if !map.vaddr.is_multiple_of(mr.page_size_bytes()) {
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
                loc_string(xml_sdf, child_pd.text_pos.unwrap())
            ));
        }
        // Also check that the child ID does not clash with any vCPU IDs, if the PD has a virtual machine
        if let Some(vm) = &pd.virtual_machine {
            for vcpu in &vm.vcpus {
                if child_id == vcpu.id {
                    return Err(format!("Error: duplicate id: {} clashes with virtual machine vcpu id in protection domain: '{}' @ {}",
                                        child_id, pd.name, loc_string(xml_sdf, child_pd.text_pos.unwrap())));
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
        Err(err) => return Err(format!("Could not parse '{filename}': {err}")),
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

    let mut pds = pd_flatten(&xml_sdf, root_pds)?;

    for node in channel_nodes {
        let ch = Channel::from_xml(&xml_sdf, &node, &pds)?;

        if let Some(setvar_id) = &ch.end_a.setvar_id {
            let setvar = SysSetVar {
                symbol: setvar_id.to_string(),
                kind: SysSetVarKind::Id { id: ch.end_a.id },
            };
            checked_add_setvar(&mut pds[ch.end_a.pd].setvars, setvar, &xml_sdf, &node)?;
        }

        if let Some(setvar_id) = &ch.end_b.setvar_id {
            let setvar = SysSetVar {
                symbol: setvar_id.to_string(),
                kind: SysSetVarKind::Id { id: ch.end_b.id },
            };
            checked_add_setvar(&mut pds[ch.end_b.pd].setvars, setvar, &xml_sdf, &node)?;
        }

        channels.push(ch);
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
        if pd.name == MONITOR_PD_NAME {
            return Err(
                "Error: the PD name 'monitor' is reserved for the Microkit Monitor.".to_string(),
            );
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

    let mut vms: Vec<&String> = vec![];
    for pd in &pds {
        if let Some(vm) = &pd.virtual_machine {
            if vms.contains(&&vm.name) {
                return Err(format!(
                    "Error: duplicate virtual machine name '{}'.",
                    vm.name
                ));
            }
            vms.push(&vm.name);
        }
    }

    // Ensure no duplicate IRQs
    let mut all_irqs = Vec::new();
    for pd in &pds {
        for sysirq in &pd.irqs {
            if all_irqs.contains(&sysirq.irq_num()) {
                return Err(format!(
                    "Error: duplicate irq: {} in protection domain: '{}' @ {}:{}:{}",
                    sysirq.irq_num(),
                    pd.name,
                    filename,
                    pd.text_pos.unwrap().row,
                    pd.text_pos.unwrap().col
                ));
            }
            all_irqs.push(sysirq.irq_num());
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
                    sysirq.id,
                    pd.name,
                    filename,
                    pd.text_pos.unwrap().row,
                    pd.text_pos.unwrap().col
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
                ch.end_a.id,
                pd.name,
                filename,
                pd.text_pos.unwrap().row,
                pd.text_pos.unwrap().col
            ));
        }

        if ch_ids[ch.end_b.pd].contains(&ch.end_b.id) {
            let pd = &pds[ch.end_b.pd];
            return Err(format!(
                "Error: duplicate channel id: {} in protection domain: '{}' @ {}:{}:{}",
                ch.end_b.id,
                pd.name,
                filename,
                pd.text_pos.unwrap().row,
                pd.text_pos.unwrap().col
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

    // Ensure no duplicate I/O Ports
    for pd in &pds {
        let mut seen_ioport_ids: Vec<u64> = Vec::new();
        for ioport in &pd.ioports {
            if seen_ioport_ids.contains(&ioport.id) {
                return Err(format!(
                    "Error: duplicate I/O port id: {} in protection domain: '{}' @ {}:{}:{}",
                    ioport.id,
                    pd.name,
                    filename,
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
    for pd in &pds {
        for this_ioport in &pd.ioports {
            for (seen_pd_name, seen_ioport) in &seen_ioports {
                let left_range = this_ioport.addr..this_ioport.addr + this_ioport.size - 1;
                let right_range = seen_ioport.addr..seen_ioport.addr + seen_ioport.size - 1;
                if ranges_overlap(&left_range, &right_range) {
                    return Err(format!(
                            "Error: I/O port id: {}, inclusive range: [{:#x}, {:#x}] in protection domain: '{}' @ {}:{}:{} overlaps with I/O port id: {}, inclusive range: [{:#x}, {:#x}] in protection domain: '{}' @ {}:{}:{}",
                            this_ioport.id,
                            left_range.start,
                            left_range.end,
                            pd.name,
                            filename,
                            this_ioport.text_pos.row,
                            this_ioport.text_pos.col,
                            seen_ioport.id,
                            right_range.start,
                            right_range.end,
                            seen_pd_name,
                            filename,
                            seen_ioport.text_pos.row,
                            seen_ioport.text_pos.col
                        ));
                }
            }
            seen_ioports.push((&pd.name, this_ioport));
        }
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
        if let SysMemoryRegionPaddr::Specified(sdf_paddr) = mr.phys_addr {
            let mr_start = sdf_paddr;
            let mr_end = sdf_paddr + mr.size;

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

    // Optimise page size of MRs, if the page size is not specified
    for mr in &mut mrs {
        if mr.page_size_specified_by_user {
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
    for pd in pds.iter() {
        for setvar in pd.setvars.iter() {
            if let SysSetVarKind::Paddr { region } = &setvar.kind {
                mr_names_with_setvar_paddr.insert(region);
            };
        }
    }
    for mr in mrs.iter_mut() {
        if mr_names_with_setvar_paddr.contains(&mr.name)
            && mr.phys_addr == SysMemoryRegionPaddr::Unspecified
        {
            // The actual allocation is done by another part of the tool.
            mr.phys_addr = SysMemoryRegionPaddr::ToolAllocated(None);
        }
    }

    Ok(SystemDescription {
        protection_domains: pds,
        memory_regions: mrs,
        channels,
    })
}

fn checked_add_setvar(
    setvars: &mut Vec<SysSetVar>,
    setvar: SysSetVar,
    xml_sdf: &XmlSystemDescription<'_>,
    node: &roxmltree::Node<'_, '_>,
) -> Result<(), String> {
    // Check that the symbol does not already exist
    for other_setvar in setvars.iter() {
        if setvar.symbol == other_setvar.symbol {
            return Err(value_error(
                xml_sdf,
                node,
                format!("setvar on symbol '{}' already exists", setvar.symbol),
            ));
        }
    }

    setvars.push(setvar);

    Ok(())
}
