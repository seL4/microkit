//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::str::FromStr;

use super::channels::Channel;
use super::consts::*;
use super::cspace::{CSpace, CapMap};
use super::domains::Domains;
use super::irq::{SysIrq, SysIrqKind};
use super::memory_region::SysMap;
use super::pci::PciDevice;
use super::util::{
    check_attributes, checked_add_setvar, checked_lookup, loc_string, sdf_parse_number, value_error,
};
use super::{SdfLocation, SdfNode, SystemDescriptionFile};

use crate::sel4::{Arch, ArmRiscvIrqTrigger, X86IoapicIrqPolarity, X86IoapicIrqTrigger};
use crate::util::str_to_bool;
use crate::Config;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CpuCore(pub u8);

impl fmt::Display for CpuCore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("cpu{:02}", self.0))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct SchedulingParams {
    pub priority: u8,
    pub budget: u64,
    pub period: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct IOPort {
    pub id: u64,
    pub addr: u64,
    pub size: u64,
    pub text_pos: SdfLocation,
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
    PrefillSize { mr: String },
}

#[derive(Debug, PartialEq, Eq)]
pub struct SysSetVar {
    pub symbol: String,
    pub kind: SysSetVarKind,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ProtectionDomain {
    /// Only populated for child protection domains
    pub id: Option<u64>,
    pub name: Rc<str>,
    pub sched_params: SchedulingParams,
    pub passive: bool,
    pub stack_size: u64,
    pub smc: bool,
    pub cpu: CpuCore,
    pub domain: Option<u8>,
    pub program_image: PathBuf,
    pub program_image_for_symbols: Option<PathBuf>,
    /// Enable FPU for this PD.
    pub fpu: bool,
    pub maps: Vec<SysMap>,
    pub irqs: Vec<SysIrq>,
    pub ioports: Vec<IOPort>,
    pub setvars: Vec<SysSetVar>,
    pub cap_maps: Vec<CapMap>,
    pub virtual_machine: Option<VirtualMachine>,
    /// Only used when parsing child PDs. All elements will be removed
    /// once we flatten each PD and its children into one list.
    pub child_pds: Vec<ProtectionDomain>,
    pub has_children: bool,
    /// Index into the total list of protection domains if a parent
    /// protection domain exists
    pub parent: Option<Rc<str>>,
    /// Value of the setvar_id attribute, if a parent protection domain exists
    pub setvar_id: Option<String>,
    /// Location in the parsed SDF file
    pub text_pos: Option<SdfLocation>,
}

impl ProtectionDomain {
    pub fn needs_ep(&self, channels: &[Channel]) -> bool {
        self.has_children
            || self.virtual_machine.is_some()
            || channels.iter().any(|channel| {
                (channel.end_a.pp && channel.end_b.pd == self.name)
                    || (channel.end_b.pp && channel.end_a.pd == self.name)
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

    pub fn priority(&self) -> u8 {
        self.sched_params.priority
    }

    pub(super) fn from_xml(
        config: &Config,
        xml_sdf: &SystemDescriptionFile,
        node: &dyn SdfNode,
        is_child: bool,
        domains: &Domains,
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
            "domain",
            "fpu",
        ];
        if is_child {
            attrs.push("id");
            attrs.push("setvar_id");
        }
        check_attributes(xml_sdf, node, &attrs)?;

        let name = Rc::from(checked_lookup(xml_sdf, node, "name")?);

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
                    config.num_cores, cpu.0
                ),
            ));
        }

        let domain = if domains.has_domains() {
            let domain_s = checked_lookup(xml_sdf, node, "domain")?;
            Some(*domains.name_to_id_map.get(domain_s).ok_or_else(|| {
                value_error(
                    xml_sdf,
                    node,
                    format!("domain '{domain_s}' not declared in <domains>:"),
                )
            })?)
        } else {
            if let Some(name) = node.attribute("domain") {
                return Err(value_error(
                    xml_sdf,
                    node,
                    format!(
                        "Specifying a domain '{name}' without declaring a \
                             domain schedule is not allowed:"
                    ),
                ));
            }

            None
        };

        #[allow(clippy::manual_range_contains)]
        if stack_size < PD_MIN_STACK_SIZE || stack_size > PD_MAX_STACK_SIZE {
            return Err(value_error(
                xml_sdf,
                node,
                format!(
                    "stack size must be between {PD_MIN_STACK_SIZE:#x} bytes and {PD_MAX_STACK_SIZE:#x} bytes"
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
        let mut program_image_for_symbols = None;
        let mut virtual_machine = None;
        let mut cspace = None;

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

        // FPU is enabled by default
        let fpu = if let Some(xml_fpu) = node.attribute("fpu") {
            match str_to_bool(xml_fpu) {
                Some(val) => val,
                None => {
                    return Err(value_error(
                        xml_sdf,
                        node,
                        "fpu must be 'true' or 'false'".to_string(),
                    ))
                }
            }
        } else {
            true
        };

        for child in node.children() {
            match child.tag_name() {
                "program_image" => {
                    check_attributes(xml_sdf, &*child, &["path", "path_for_symbols"])?;
                    if program_image.is_some() {
                        return Err(value_error(
                            xml_sdf,
                            node,
                            "program_image must only be specified once".to_string(),
                        ));
                    }

                    let program_image_path = checked_lookup(xml_sdf, &*child, "path")?;
                    program_image = Some(Path::new(program_image_path).to_path_buf());

                    program_image_for_symbols =
                        child.attribute("path_for_symbols").map(PathBuf::from);
                }
                "map" => {
                    let map_max_vaddr = config.pd_map_max_vaddr(stack_size);
                    let map = SysMap::from_xml(xml_sdf, &*child, true, map_max_vaddr)?;

                    if let Some(setvar_vaddr) = child.attribute("setvar_vaddr") {
                        let setvar = SysSetVar {
                            symbol: setvar_vaddr.to_string(),
                            kind: SysSetVarKind::Vaddr { address: map.vaddr },
                        };
                        checked_add_setvar(&mut setvars, setvar, xml_sdf, &*child)?;
                    }

                    if let Some(setvar_size) = child.attribute("setvar_size") {
                        let setvar = SysSetVar {
                            symbol: setvar_size.to_string(),
                            kind: SysSetVarKind::Size { mr: map.mr.clone() },
                        };
                        checked_add_setvar(&mut setvars, setvar, xml_sdf, &*child)?;
                    }

                    if let Some(setvar_prefill_size) = child.attribute("setvar_prefill_size") {
                        let setvar = SysSetVar {
                            symbol: setvar_prefill_size.to_string(),
                            kind: SysSetVarKind::PrefillSize { mr: map.mr.clone() },
                        };
                        checked_add_setvar(&mut setvars, setvar, xml_sdf, &*child)?;
                    }

                    maps.push(map);
                }
                "irq" => {
                    let id = checked_lookup(xml_sdf, &*child, "id")?
                        .parse::<i64>()
                        .unwrap();
                    if id > PD_MAX_ID as i64 {
                        return Err(value_error(
                            xml_sdf,
                            &*child,
                            format!("id must be < {}", PD_MAX_ID + 1),
                        ));
                    }
                    if id < 0 {
                        return Err(value_error(xml_sdf, &*child, "id must be >= 0".to_string()));
                    }

                    if let Some(setvar_id) = child.attribute("setvar_id") {
                        let setvar = SysSetVar {
                            symbol: setvar_id.to_string(),
                            kind: SysSetVarKind::Id { id: id as u64 },
                        };
                        checked_add_setvar(&mut setvars, setvar, xml_sdf, &*child)?;
                    }

                    if let Some(irq_str) = child.attribute("irq") {
                        if config.arch == Arch::X86_64 {
                            return Err(value_error(
                                xml_sdf,
                                &*child,
                                "ARM and RISC-V IRQs are not supported on x86".to_string(),
                            ));
                        }

                        // ARM and RISC-V interrupts must have an "irq" attribute.
                        check_attributes(xml_sdf, &*child, &["irq", "id", "setvar_id", "trigger"])?;
                        let irq = irq_str.parse::<u64>().unwrap();
                        let trigger = if let Some(trigger_str) = child.attribute("trigger") {
                            match trigger_str {
                                "level" => ArmRiscvIrqTrigger::Level,
                                "edge" => ArmRiscvIrqTrigger::Edge,
                                _ => {
                                    return Err(value_error(
                                        xml_sdf,
                                        &*child,
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
                                &*child,
                                "x86 I/O APIC IRQ isn't supported on ARM and RISC-V".to_string(),
                            ));
                        }

                        // IOAPIC interrupts (X86_64) must have a "pin" attribute.
                        check_attributes(
                            xml_sdf,
                            &*child,
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
                                &*child,
                                "ioapic must be >= 0".to_string(),
                            ));
                        }

                        let pin = pin_str.parse::<i64>().unwrap();
                        if pin < 0 {
                            return Err(value_error(
                                xml_sdf,
                                &*child,
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
                                        &*child,
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
                                        &*child,
                                        "polarity must be either 'low' or 'high'".to_string(),
                                    ))
                                }
                            }
                        } else {
                            // Default to normal polarity
                            X86IoapicIrqPolarity::HighTriggered
                        };
                        let vector = checked_lookup(xml_sdf, &*child, "vector")?
                            .parse::<i64>()
                            .unwrap();
                        if !(0..=X86_IRQ_VECTOR_MAX).contains(&vector) {
                            return Err(value_error(
                                xml_sdf,
                                &*child,
                                format!("vector must be within [0..{X86_IRQ_VECTOR_MAX}]"),
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
                                &*child,
                                "x86 MSI IRQ isn't supported on ARM and RISC-V".to_string(),
                            ));
                        }

                        // MSI interrupts (X86_64) have a "pcidev" attribute.
                        check_attributes(
                            xml_sdf,
                            &*child,
                            &["id", "setvar_id", "pcidev", "handle", "vector"],
                        )?;

                        let pci_device = PciDevice::from_str(pcidev_str)
                            .map_err(|err| value_error(xml_sdf, &*child, err.to_string()))?;

                        let handle = checked_lookup(xml_sdf, &*child, "handle")?
                            .parse::<i64>()
                            .unwrap();
                        if handle < 0 {
                            return Err(value_error(
                                xml_sdf,
                                &*child,
                                "handle must be >= 0".to_string(),
                            ));
                        }

                        let vector = checked_lookup(xml_sdf, &*child, "vector")?
                            .parse::<i64>()
                            .unwrap();
                        if !(0..=X86_IRQ_VECTOR_MAX).contains(&vector) {
                            return Err(value_error(
                                xml_sdf,
                                &*child,
                                format!("vector must be within [0..{X86_IRQ_VECTOR_MAX}]"),
                            ));
                        }

                        let irq = SysIrq {
                            id: id as u64,
                            kind: SysIrqKind::MSI {
                                pci_device,
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
                                checked_lookup(xml_sdf, &*child, "irq")?
                            }
                            Arch::X86_64 => {
                                checked_lookup(xml_sdf, &*child, "pin")?;
                                checked_lookup(xml_sdf, &*child, "pcidev")?
                            }
                        };
                    }
                }
                "ioport" => {
                    if let Arch::X86_64 = config.arch {
                        check_attributes(
                            xml_sdf,
                            &*child,
                            &["id", "setvar_id", "setvar_addr", "addr", "size"],
                        )?;

                        let id = checked_lookup(xml_sdf, &*child, "id")?
                            .parse::<i64>()
                            .unwrap();
                        if id > PD_MAX_ID as i64 {
                            return Err(value_error(
                                xml_sdf,
                                &*child,
                                format!("id must be < {}", PD_MAX_ID + 1),
                            ));
                        }
                        if id < 0 {
                            return Err(value_error(
                                xml_sdf,
                                &*child,
                                "id must be >= 0".to_string(),
                            ));
                        }

                        if let Some(setvar_id) = child.attribute("setvar_id") {
                            let setvar = SysSetVar {
                                symbol: setvar_id.to_string(),
                                kind: SysSetVarKind::Id { id: id as u64 },
                            };
                            checked_add_setvar(&mut setvars, setvar, xml_sdf, &*child)?;
                        }

                        let addr =
                            sdf_parse_number(checked_lookup(xml_sdf, &*child, "addr")?, &*child)?;

                        if let Some(setvar_addr) = child.attribute("setvar_addr") {
                            let setvar = SysSetVar {
                                symbol: setvar_addr.to_string(),
                                kind: SysSetVarKind::X86IoPortAddr { address: addr },
                            };
                            checked_add_setvar(&mut setvars, setvar, xml_sdf, &*child)?;
                        }

                        let size = checked_lookup(xml_sdf, &*child, "size")?
                            .parse::<i64>()
                            .unwrap();
                        if size <= 0 {
                            return Err(value_error(
                                xml_sdf,
                                &*child,
                                "size must be > 0".to_string(),
                            ));
                        }

                        ioports.push(IOPort {
                            id: id as u64,
                            addr,
                            size: size as u64,
                            text_pos: node.range().start,
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
                    check_attributes(xml_sdf, &*child, &["symbol", "region_paddr"])?;
                    let symbol = checked_lookup(xml_sdf, &*child, "symbol")?.to_string();
                    let region = checked_lookup(xml_sdf, &*child, "region_paddr")?.to_string();
                    let setvar = SysSetVar {
                        symbol,
                        kind: SysSetVarKind::Paddr { region },
                    };
                    checked_add_setvar(&mut setvars, setvar, xml_sdf, &*child)?;
                }
                "protection_domain" => {
                    let child_pd =
                        ProtectionDomain::from_xml(config, xml_sdf, &*child, true, domains)?;

                    if let Some(setvar_id) = child_pd.setvar_id.clone() {
                        let setvar = SysSetVar {
                            symbol: setvar_id.to_string(),
                            kind: SysSetVarKind::Id {
                                id: child_pd.id.unwrap(),
                            },
                        };
                        checked_add_setvar(&mut setvars, setvar, xml_sdf, &*child)?;
                    }

                    child_pds.push(child_pd);
                }
                "virtual_machine" => {
                    if !config.hypervisor {
                        return Err(value_error(
                            xml_sdf,
                            node,
                            "seL4 has not been built as a hypervisor, virtual machines are disabled".to_string()
                        ));
                    }
                    if virtual_machine.is_some() {
                        return Err(value_error(
                            xml_sdf,
                            node,
                            "virtual_machine must only be specified once".to_string(),
                        ));
                    }

                    let vm = VirtualMachine::from_xml(config, xml_sdf, &*child)?;

                    for vcpu in &vm.vcpus {
                        if let Some(setvar_id) = &vcpu.setvar_id {
                            let setvar = SysSetVar {
                                symbol: setvar_id.to_string(),
                                kind: SysSetVarKind::Id { id: vcpu.id },
                            };
                            checked_add_setvar(&mut setvars, setvar, xml_sdf, &*child)?;
                        }
                    }

                    virtual_machine = Some(vm);
                }
                "cspace" => {
                    if cspace.is_some() {
                        return Err(value_error(
                            xml_sdf,
                            node,
                            "cspace must only be specified once".to_string(),
                        ));
                    }

                    cspace = Some(CSpace::from_xml(xml_sdf, &*child)?);
                }
                _ => {
                    let pos = child.range().start;
                    return Err(format!(
                        "Invalid XML element '{}': {}",
                        child.tag_name(),
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
            sched_params: SchedulingParams {
                // This downcast is safe as we have checked that this is less than
                // the maximum PD priority, which fits in a u8.
                priority: priority as u8,
                budget,
                period,
            },
            passive,
            stack_size,
            smc,
            cpu,
            domain,
            program_image: program_image.unwrap(),
            program_image_for_symbols,
            fpu,
            maps,
            irqs,
            ioports,
            setvars,
            cap_maps: cspace.map(|cspace| cspace.cap_maps).unwrap_or_default(),
            child_pds,
            virtual_machine,
            has_children,
            parent: None,
            setvar_id,
            text_pos: Some(node.range().start),
        })
    }
}

/// Given an iterable of protection domains flatten the tree representation
/// into a flat tuple.
///
/// In doing so the representation is changed from "Node with list of children",
/// to each node having a parent link instead.
pub fn pd_flatten(
    xml_sdf: &SystemDescriptionFile,
    pds: Vec<ProtectionDomain>,
) -> Result<Vec<ProtectionDomain>, String> {
    let mut all_pds = vec![];

    for pd in pds {
        // These are all root PDs, so should not have parents.
        assert!(pd.parent.is_none());
        // We provide the index of the PD in the entire PD list
        all_pds.extend(pd_tree_to_list(xml_sdf, pd)?);
    }

    Ok(all_pds)
}

/// Take a PD and return a vector with the given PD at the start and all of the children PDs following.
///
/// For example if PD A had children B, C then we would have [A, B, C].
/// If we had the same example but child B also had a child D, we would have [A, B, D, C].
fn pd_tree_to_list(
    xml_sdf: &SystemDescriptionFile,
    mut pd: ProtectionDomain,
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
        child_pd.parent = Some(pd.name.clone());
        new_child_pds.extend(pd_tree_to_list(xml_sdf, child_pd)?);
    }

    let mut all = vec![pd];
    all.extend(new_child_pds);

    Ok(all)
}

#[derive(Debug, PartialEq, Eq)]
pub struct VirtualMachine {
    pub vcpus: Vec<VirtualCpu>,
    pub name: Rc<str>,
    pub maps: Vec<SysMap>,
    pub sched_params: Option<SchedulingParams>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct VirtualCpu {
    pub id: u64,
    pub setvar_id: Option<String>,
    pub cpu: Option<CpuCore>,
}

impl VirtualMachine {
    fn from_xml(
        config: &Config,
        xml_sdf: &SystemDescriptionFile,
        node: &dyn SdfNode,
    ) -> Result<VirtualMachine, String> {
        if config.arch == Arch::Aarch64 {
            check_attributes(xml_sdf, node, &["name", "budget", "period", "priority"])?;
        } else {
            check_attributes(xml_sdf, node, &["name"])?;
        }

        let name = Rc::from(checked_lookup(xml_sdf, node, "name")?);

        let sched_params = if config.arch == Arch::Aarch64 {
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

            Some(SchedulingParams {
                // This downcast is safe as we have checked that this is less than
                // the maximum PD priority, which fits in a u8.
                priority: priority as u8,
                budget,
                period,
            })
        } else {
            None
        };

        let mut vcpus: Vec<VirtualCpu> = Vec::new();
        let mut maps = Vec::new();
        for child in node.children() {
            let child_name = child.tag_name();
            match child_name {
                "vcpu" => {
                    check_attributes(xml_sdf, &*child, &["id", "setvar_id", "cpu"])?;
                    let id = checked_lookup(xml_sdf, &*child, "id")?
                        .parse::<u64>()
                        .unwrap();
                    if id > VCPU_MAX_ID {
                        return Err(value_error(
                            xml_sdf,
                            &*child,
                            format!("id must be < {}", VCPU_MAX_ID + 1),
                        ));
                    }

                    for vcpu in &vcpus {
                        if vcpu.id == id {
                            let pos = child.range().start;
                            return Err(format!(
                                "Error: duplicate vcpu id {} in virtual machine '{}' @ {}",
                                id,
                                name,
                                loc_string(xml_sdf, pos)
                            ));
                        }
                    }

                    let setvar_id = node.attribute("setvar_id").map(ToOwned::to_owned);

                    let cpu = if let Some(cpu) = child.attribute("cpu") {
                        let cpu_value: u8 = sdf_parse_number(cpu, node)?
                            .try_into()
                            .expect("cpu # fits in u8");

                        if cpu_value >= config.num_cores {
                            return Err(value_error(
                                xml_sdf,
                                &*child,
                                format!(
                                    "cpu core must be less than {}, got {}",
                                    config.num_cores, cpu_value
                                ),
                            ));
                        }

                        Some(CpuCore(cpu_value))
                    } else {
                        None
                    };

                    vcpus.push(VirtualCpu { id, setvar_id, cpu });
                }
                "map" => {
                    // Virtual machines do not have program images and so we do not allow
                    // setvar_vaddr on SysMap
                    let map = SysMap::from_xml(xml_sdf, &*child, false, config.vm_map_max_vaddr())?;
                    maps.push(map);
                }
                _ => {
                    let pos = node.range().start;
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
            sched_params,
        })
    }
}
