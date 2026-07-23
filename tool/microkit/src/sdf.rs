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

use crate::util::{calculate_size_bits, get_full_path, ranges_overlap, round_up, str_to_bool};
use crate::MAX_PDS;
use sel4_capdl_initializer_types::{
    object, x86_io_address_space, DomainSchedDuration, DomainSchedEntry, FillEntryContentBootInfoId,
};
use std::collections::{hash_map, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::num::NonZero;
use std::ops::Deref;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::str::FromStr;

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
pub const MONITOR_DOMAIN: u8 = 0;

/// Default to a stack size of 8KiB
pub const PD_DEFAULT_STACK_SIZE: u64 = 0x2000;
const PD_MIN_STACK_SIZE: u64 = 0x1000;
const PD_MAX_STACK_SIZE: u64 = 1024 * 1024 * 16;

/// Maximum x86 IRQ vector value. Inclusive.
/// This value is calculated by the kernel as `irq_user_max - irq_user_min` in
/// `src/arch/x86/object/interrupt.c`
const X86_IRQ_VECTOR_MAX: i64 = 107;

/// The purpose of this function is to parse an integer that could
/// either be in decimal or hex format, unlike the normal parsing
/// functionality that the Rust standard library provides.
/// This also removes any underscores that may be present in the number
/// Always returns a base 10 integer.
fn sdf_parse_number(s: &str, node: &dyn SdfNode) -> Result<u64, String> {
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
            node.tag_name(),
            err
        )),
    }
}

fn loc_string(xml_sdf: &XmlSystemDescription, pos: SdfLocation) -> String {
    format!("{}:{}:{}", xml_sdf.filename.display(), pos.row, pos.col)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SdfLocation {
    pub row: u32,
    pub col: u32,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciDevice(pub object::PCIDevice);

impl Deref for PciDevice {
    type Target = object::PCIDevice;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<PciDevice> for object::PCIDevice {
    fn from(device: PciDevice) -> Self {
        device.0
    }
}

impl From<object::PCIDevice> for PciDevice {
    fn from(device: object::PCIDevice) -> Self {
        PciDevice(device)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciDeviceParseError {
    Malformed,
    BusParse,
    DeviceParse,
    FunctionParse,
    BusOutOfRange,
    DeviceOutOfRange,
    FunctionOutOfRange,
}

impl fmt::Display for PciDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}.{:x}",
            self.bus, self.device, self.function
        )
    }
}

impl fmt::Display for PciDeviceParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PciDeviceParseError::Malformed => {
                write!(f, "expected PCI address in bus:device.function form")
            }
            PciDeviceParseError::BusParse => write!(f, "failed to parse PCI bus"),
            PciDeviceParseError::DeviceParse => write!(f, "failed to parse PCI device"),
            PciDeviceParseError::FunctionParse => write!(f, "failed to parse PCI function"),
            PciDeviceParseError::BusOutOfRange => {
                write!(
                    f,
                    "PCI bus must be within [0..{}]",
                    object::PCIDevice::PCI_BUS_MAX
                )
            }
            PciDeviceParseError::DeviceOutOfRange => {
                write!(
                    f,
                    "PCI device must be within [0..{}]",
                    object::PCIDevice::PCI_DEV_MAX
                )
            }
            PciDeviceParseError::FunctionOutOfRange => {
                write!(
                    f,
                    "PCI function must be within [0..{}]",
                    object::PCIDevice::PCI_FUNC_MAX
                )
            }
        }
    }
}

impl FromStr for PciDevice {
    type Err = PciDeviceParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (bus_str, device_function_str) =
            s.split_once(':').ok_or(PciDeviceParseError::Malformed)?;
        let (device_str, function_str) = device_function_str
            .split_once('.')
            .ok_or(PciDeviceParseError::Malformed)?;

        let bus = i64::from_str_radix(bus_str.trim(), 16)
            .map_err(|_| PciDeviceParseError::BusParse)
            .and_then(|bus| {
                match (0..=i64::from(object::PCIDevice::PCI_BUS_MAX)).contains(&bus) {
                    true => Ok(bus as u8),
                    false => Err(PciDeviceParseError::BusOutOfRange),
                }
            })?;
        let device = i64::from_str_radix(device_str.trim(), 16)
            .map_err(|_| PciDeviceParseError::DeviceParse)
            .and_then(|device| {
                match (0..=i64::from(object::PCIDevice::PCI_DEV_MAX)).contains(&device) {
                    true => Ok(device as u8),
                    false => Err(PciDeviceParseError::DeviceOutOfRange),
                }
            })?;
        let function = i64::from_str_radix(function_str.trim(), 16)
            .map_err(|_| PciDeviceParseError::FunctionParse)
            .and_then(|function| {
                match (0..=i64::from(object::PCIDevice::PCI_FUNC_MAX)).contains(&function) {
                    true => Ok(function as u8),
                    false => Err(PciDeviceParseError::FunctionOutOfRange),
                }
            })?;

        let result = object::PCIDevice {
            bus,
            device,
            function,
        };
        Ok(result.into())
    }
}

// This can be extended in future to support devices on an SMMU enabled Arm device
// or IOMMU enabled RISC-V device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IommuDeviceIdentifier {
    X86Pci(PciDevice),
}

#[derive(Clone, PartialEq, Eq)]
pub enum IommuDeviceIdentifierParseError {
    UnsupportedArch(Arch),
    Pci(PciDeviceParseError),
}

impl IommuDeviceIdentifier {
    fn from_str_for_arch(
        config: &Config,
        s: &str,
    ) -> Result<Self, IommuDeviceIdentifierParseError> {
        match config.arch {
            Arch::X86_64 => PciDevice::from_str(s)
                .map(IommuDeviceIdentifier::X86Pci)
                .map_err(IommuDeviceIdentifierParseError::Pci),
            Arch::Aarch64 | Arch::Riscv64 => Err(IommuDeviceIdentifierParseError::UnsupportedArch(
                config.arch,
            )),
        }
    }
}

impl fmt::Display for IommuDeviceIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IommuDeviceIdentifier::X86Pci(pci_device) => write!(f, "PCI device {pci_device}"),
        }
    }
}

impl fmt::Display for IommuDeviceIdentifierParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IommuDeviceIdentifierParseError::UnsupportedArch(arch) => {
                write!(f, "IOMMU device identifiers are not supported on {arch}")
            }
            IommuDeviceIdentifierParseError::Pci(err) => write!(f, "{err}"),
        }
    }
}

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
    page_size_specified_by_user: bool,
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
        pci_device: PciDevice,
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

impl fmt::Display for CpuCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
pub struct ProtectionDomain {
    /// Only populated for child protection domains
    pub id: Option<u64>,
    pub name: String,
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
    pub cspace: Option<CSpace>,
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
    text_pos: Option<SdfLocation>,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
pub enum CapMapType {
    Tcb,
    Sc,
    VSpace,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CapMap {
    pub cap_type: CapMapType,
    // FIXME: This is quite a hack. Basically, we need to be able to reference
    // arbitrary PDs, but to gather the index, we need to know all the PDs.
    // However, at the time of parsing the cap maps, we are in the process
    // of parsing all the PDs. In lieu of something better (in my - @midnightveil's
    // opinion, making everything think in terms of PD names, and something
    // that was necessary to do for the multikernel changes); the pd idx will
    // be filled out later during SDF parse process.
    pub pd_name: String,
    pub pd: Option<usize>,
    // The destination "slot" in the CSpace: note that this is "opaque" and
    // can be shifted depending on the location in the CSpace to work as the CPtr,
    // but here it is given as the index into the CNode.
    pub slot: u64,
    /// Location in the parsed SDF file
    text_pos: SdfLocation,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CSpace {
    pub cap_maps: Vec<CapMap>,
    pub size_bits: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct VirtualMachine {
    pub vcpus: Vec<VirtualCpu>,
    pub name: String,
    pub maps: Vec<SysMap>,
    pub sched_params: Option<SchedulingParams>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct VirtualCpu {
    pub id: u64,
    pub setvar_id: Option<String>,
    pub cpu: Option<CpuCore>,
}

impl SysMap {
    fn from_xml(
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
    fn from_xml(
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

// This is implemented in such a way that each device will have its own address space.
// If devices need to share physical memory, this can be done by mapping the same memory_region
// into each address space.
struct IOAddressSpace {
    iomaps: Vec<SysIOMap>,
}

impl IOAddressSpace {
    fn from_xml(
        config: &Config,
        xml_sdf: &XmlSystemDescription,
        node: &dyn SdfNode,
        names: &mut HashSet<String>,
        domain_ids: &mut HashSet<u64>,
        iommu_device_identifiers: &mut Vec<IommuDeviceIdentifier>,
    ) -> Result<IOAddressSpace, String> {
        if !config.iommu {
            let pos = node.range().start;
            return Err(format!(
                "Error: io address space requires seL4 to be built with IOMMU support: {}",
                loc_string(xml_sdf, pos)
            ));
        }

        check_attributes(xml_sdf, node, &["name", "peripheral_id", "domain_id"])?;
        let name = checked_lookup(xml_sdf, node, "name")?;
        if !names.insert(name.to_string()) {
            return Err(value_error(
                xml_sdf,
                node,
                format!("duplicate name '{name}'"),
            ));
        }

        // Currently we enforce unqiue domain ids. To support shared domain ids, we have to ensure
        // each device has a duplicated copy of the whole page table structure due to how Intel
        // implements IOMMU caching see section 6.2.1 in the Virtualization Technology (Intel® VT) for Directed I/O
        // (Intel® VT-d) manual:
        // http://www.intel.com/content/dam/www/public/us/en/documents/product-specifications/vt-directed-io-spec.pdf
        let domain_id = match config.arch {
            Arch::X86_64 => {
                let domain_id =
                    sdf_parse_number(checked_lookup(xml_sdf, node, "domain_id")?, node)?;
                if !domain_ids.insert(domain_id) {
                    return Err(value_error(
                        xml_sdf,
                        node,
                        "reusing a domain id is forbidden".into(),
                    ));
                }
                Some(domain_id)
            }
            _ => None,
        };

        // In the SDF we use peripheral_id as an architecture agnostic way to describe
        // how a device is identified in a system. For example on x86 the IOMMU identifies
        // devices by the PCI tuple (bus,dev,fn)
        let identifier_str = checked_lookup(xml_sdf, node, "peripheral_id")?;
        let identifier =
            IommuDeviceIdentifier::from_str_for_arch(config, identifier_str).map_err(|err| {
                value_error(
                    xml_sdf,
                    node,
                    format!("failed to parse device peripheral_id '{identifier_str}': {err}"),
                )
            })?;
        if iommu_device_identifiers.contains(&identifier) {
            return Err(value_error(
                xml_sdf,
                node,
                format!("duplicate device peripheral_id '{identifier}'"),
            ));
        }
        iommu_device_identifiers.push(identifier);

        let mut iomaps = Vec::new();

        for child in node.children() {
            match child.tag_name() {
                "iomap" => {
                    let iomap =
                        SysIOMap::from_xml(config, xml_sdf, &*child, name, identifier, domain_id)?;
                    iomaps.push(iomap);
                }
                _ => {
                    let pos = child.range().start;
                    return Err(format!(
                        "Error: invalid XML element '{}': {}",
                        child.tag_name(),
                        loc_string(xml_sdf, pos)
                    ));
                }
            }
        }

        Ok(IOAddressSpace { iomaps })
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

    pub fn priority(&self) -> u8 {
        self.sched_params.priority
    }

    fn from_xml(
        config: &Config,
        xml_sdf: &XmlSystemDescription,
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
            cspace,
            child_pds,
            virtual_machine,
            has_children,
            parent: None,
            setvar_id,
            text_pos: Some(node.range().start),
        })
    }
}

impl VirtualMachine {
    fn from_xml(
        config: &Config,
        xml_sdf: &XmlSystemDescription,
        node: &dyn SdfNode,
    ) -> Result<VirtualMachine, String> {
        if config.arch == Arch::Aarch64 {
            check_attributes(xml_sdf, node, &["name", "budget", "period", "priority"])?;
        } else {
            check_attributes(xml_sdf, node, &["name"])?;
        }

        let name = checked_lookup(xml_sdf, node, "name")?.to_string();

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

impl CapMap {
    fn from_xml(
        cap_type: CapMapType,
        xml_sdf: &XmlSystemDescription,
        node: &dyn SdfNode,
    ) -> Result<CapMap, String> {
        // At the moment the four cap maps we support all have the 'pd' element,
        // so we can include it here. When that stops being the case we will
        // have to rework this a bit.
        check_attributes(xml_sdf, node, &["slot", "pd"])?;

        let pd_name = checked_lookup(xml_sdf, node, "pd")?.to_string();

        let slot = sdf_parse_number(checked_lookup(xml_sdf, node, "slot")?, node)?;

        if slot == 0 {
            return Err(value_error(
                xml_sdf,
                node,
                ("The destination slot 0 has been reserved for Microkit CNode").to_string(),
            ));
        }

        Ok(CapMap {
            cap_type,
            pd_name,
            // FIXME: Hack, filled out later.
            pd: None,
            slot,
            text_pos: node.range().start,
        })
    }
}

impl CSpace {
    fn from_xml(xml_sdf: &XmlSystemDescription, node: &dyn SdfNode) -> Result<Self, String> {
        check_attributes(xml_sdf, node, &[])?;

        let mut cap_maps = vec![];

        for child in node.children() {
            cap_maps.push(match child.tag_name() {
                "cap_tcb" => CapMap::from_xml(CapMapType::Tcb, xml_sdf, &*child)?,
                "cap_sc" => CapMap::from_xml(CapMapType::Sc, xml_sdf, &*child)?,
                "cap_vspace" => CapMap::from_xml(CapMapType::VSpace, xml_sdf, &*child)?,
                child_name => {
                    let location = loc_string(xml_sdf, child.range().start);
                    if let Some(type_name) = child_name.strip_prefix("cap_") {
                        return Err(format!("Cap type: '{type_name}' is not supported at '{location}'"));
                    } else {
                        return Err(format!("Element '{child_name}' is not supported in a <cspace> element at '{location}'"));
                    }
                }
            })
        }

        // Default to 1, the minimum allowed by the kernel.
        let size_bits = cap_maps
            .iter()
            .map(|cap_map| calculate_size_bits(cap_map.slot + 1))
            .max()
            .unwrap_or(1) as u64;

        Ok(CSpace {
            cap_maps,
            size_bits,
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

    fn from_xml(
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

impl ChannelEnd {
    fn from_xml<'a>(
        xml_sdf: &'a XmlSystemDescription,
        node: &'a dyn SdfNode,
        pds: &[ProtectionDomain],
    ) -> Result<ChannelEnd, String> {
        let node_name = node.tag_name();
        if node_name != "end" {
            let pos = node.range().start;
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
        node: &'a dyn SdfNode,
        pds: &[ProtectionDomain],
    ) -> Result<Channel, String> {
        check_attributes(xml_sdf, node, &[])?;

        let [ref end_a, ref end_b] = node
            .children()
            .map(|node| ChannelEnd::from_xml(xml_sdf, &*node, pds))
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

#[derive(Debug, Default)]
pub struct Domains {
    pub name_to_id_map: HashMap<String, u8>,
    pub schedule_set_start: Option<u64>,
    pub schedule_index_shift: Option<u64>,
    pub schedule: Vec<DomainSchedEntry>,
}

impl Domains {
    fn from_xml(
        config: &Config,
        xml_sdf: &XmlSystemDescription,
        node: &dyn SdfNode,
    ) -> Result<Self, String> {
        check_attributes(xml_sdf, node, &[])?;

        if config.num_cores != 1 {
            return Err(
                "Error: The domain scheduler is not supported in multicore builds of seL4"
                    .to_string(),
            );
        }

        let mut name_to_id_map = HashMap::<String, Option<u8>>::new();
        let mut id_to_name_map = HashMap::<u8, String>::new();
        let mut domain_schedule_element = None;

        for child in node.children() {
            match child.tag_name() {
                "domain" => {
                    let (dom_name, dom_id) = Self::domain_from_xml(config, xml_sdf, &*child)?;

                    if let Some(existing_dom) = name_to_id_map.insert(dom_name.clone(), dom_id) {
                        return Err(value_error(
                            xml_sdf,
                            &*child,
                            format!(
                                "Each <domain>'s name element must be unique \
                                 found existing domain '{dom_name}' with id '{existing_dom:?}'"
                            ),
                        ));
                    }

                    if let Some(dom_id) = dom_id {
                        if let Some(existing_dom) = id_to_name_map.insert(dom_id, dom_name.clone())
                        {
                            return Err(value_error(
                                xml_sdf,
                                &*child,
                                format!(
                                    "Each <domain>'s id element must be unique \
                                     found existing domain '{existing_dom}' with id '{dom_id}'"
                                ),
                            ));
                        }
                    }
                }
                "domain_schedule" => {
                    if domain_schedule_element.is_some() {
                        return Err(value_error(
                            xml_sdf,
                            &*child,
                            "The <domain_schedule> element can only appear once".to_string(),
                        ));
                    }

                    domain_schedule_element = Some(child);
                }
                _ => {
                    let pos = child.range().start;
                    return Err(format!(
                        "Error: invalid XML element as child of <domains> '{}': {}",
                        child.tag_name(),
                        loc_string(xml_sdf, pos)
                    ));
                }
            }
        }

        let Some(domain_schedule_element) = domain_schedule_element else {
            return Err(value_error(
                xml_sdf,
                node,
                "The <domain_schedule> element must appear once".to_string(),
            ));
        };

        let name_to_id_map = name_to_id_map
            .into_iter()
            .map(|(name, dom)| match dom {
                Some(dom) => Ok((name, dom)),
                None => {
                    // TODO: We could be more efficient here. However, for a
                    // maximum of 256 possible domains, iterating over the
                    // valid possible domain IDs is actually OK.

                    let mut dom = None;
                    for i in 0..=config.num_domains {
                        if let hash_map::Entry::Vacant(e) = id_to_name_map.entry(i) {
                            e.insert(name.clone());
                            dom = Some(i);
                            break;
                        }
                    }

                    let Some(dom) = dom else {
                        return Err(value_error(
                            xml_sdf,
                            node,
                            format!("Number of domains exceeds {}", config.num_domains),
                        ));
                    };

                    Ok((name, dom))
                }
            })
            .collect::<Result<_, _>>()?;

        Self::domain_schedule_from_xml(config, xml_sdf, &*domain_schedule_element, name_to_id_map)
    }

    fn domain_from_xml(
        config: &Config,
        xml_sdf: &XmlSystemDescription,
        node: &dyn SdfNode,
    ) -> Result<(String, Option<u8>), String> {
        check_attributes(xml_sdf, node, &["name", "id"])?;

        let name = checked_lookup(xml_sdf, node, "name")?.to_string();

        let domain_id = node
            .attribute("id")
            .map(|s| sdf_parse_number(s, node))
            .transpose()?
            .map(|n| {
                if n >= config.num_domains.into() {
                    Err(value_error(
                        xml_sdf,
                        node,
                        format!(
                            "domain id {n} should be less than the \
                             configured KernelNumDomains value of {}",
                            config.num_domains
                        ),
                    ))
                } else {
                    Ok(n.try_into()
                        .expect("num_domains is u8 so by if above this is OK"))
                }
            })
            .transpose()?;

        Ok((name, domain_id))
    }

    fn domain_schedule_from_xml(
        config: &Config,
        xml_sdf: &XmlSystemDescription,
        node: &dyn SdfNode,
        name_to_id_map: HashMap<String, u8>,
    ) -> Result<Domains, String> {
        check_attributes(xml_sdf, node, &["index_shift", "start_index"])?;

        let schedule_start_index = node
            .attribute("start_index")
            .map(|s| sdf_parse_number(s, node))
            .transpose()?
            // The domain schedule is only started when the start index is Some(...)
            // so even when not specified we default to a start index of zero.
            .unwrap_or(0);

        let schedule_index_shift = node
            .attribute("index_shift")
            .map(|s| sdf_parse_number(s, node))
            .transpose()?;

        let mut schedule = vec![];

        for child in node.children() {
            match child.tag_name() {
                "schedule_entry" => {
                    schedule.push(Self::schedule_entry_from_xml(
                        xml_sdf,
                        &*child,
                        &name_to_id_map,
                    )?);
                }
                "schedule_end_marker" => {
                    check_attributes(xml_sdf, &*child, &[])?;

                    schedule.push(DomainSchedEntry {
                        domain: 0,
                        duration: DomainSchedDuration::EndMarker,
                    });
                }
                name => {
                    let pos = child.range().start;
                    return Err(format!(
                        "Error: invalid XML element as child of <domain_schedule> '{name}': {}",
                        loc_string(xml_sdf, pos)
                    ));
                }
            }
        }

        if schedule.len() >= config.num_domain_schedules.try_into().unwrap() {
            return Err(format!(
                "More than configured KernelNumDomainSchedules {} \
                number of <schedule_entry> elements found",
                config.num_domain_schedules
            ));
        }

        if schedule_start_index >= schedule.len().try_into().unwrap() {
            return Err(value_error(
                xml_sdf,
                node,
                format!(
                    "schedule_start_index '{schedule_start_index}' is \
                     greater than the length of the schedule '{}'",
                    schedule.len()
                ),
            ));
        }

        if let Some(shift) = schedule_index_shift {
            if shift + u64::try_from(schedule.len()).unwrap() >= config.num_domain_schedules {
                return Err(value_error(
                    xml_sdf,
                    node,
                    format!(
                        "schedule_index_shift '{schedule_start_index}' on top of \
                         the schedule length '{}' would exceed than the configured \
                         KernelNumDomainSchedules {}",
                        schedule.len(),
                        config.num_domain_schedules
                    ),
                ));
            }
        }

        Ok(Domains {
            name_to_id_map,
            schedule_set_start: Some(schedule_start_index),
            schedule_index_shift,
            schedule,
        })
    }

    fn schedule_entry_from_xml(
        xml_sdf: &XmlSystemDescription,
        node: &dyn SdfNode,
        name_to_id_map: &HashMap<String, u8>,
    ) -> Result<DomainSchedEntry, String> {
        check_attributes(xml_sdf, node, &["domain", "duration"])?;

        let domain_name = checked_lookup(xml_sdf, node, "domain")?;
        let duration_str = checked_lookup(xml_sdf, node, "duration")?;

        let &domain = name_to_id_map.get(domain_name).ok_or_else(|| {
            value_error(
                xml_sdf,
                node,
                format!("domain '{domain_name}' does not exist,"),
            )
        })?;

        let (duration_raw, duration_unit) = duration_str.split_once(" ").ok_or_else(|| {
            value_error(
                xml_sdf,
                node,
                format!(
                    "The duration '{duration_str}' must contain a value and a unit, e.g. '1000 us'"
                ),
            )
        })?;

        let duration_int = sdf_parse_number(duration_raw, node)?;
        let duration = NonZero::new(duration_int).ok_or_else(|| {
            value_error(
                xml_sdf,
                node,
                format!("The duration '{duration_str}' must be non-zero"),
            )
        })?;

        let duration = match duration_unit {
            "us" => Ok(DomainSchedDuration::Us(duration)),
            "ticks" => Ok(DomainSchedDuration::Ticks(duration)),
            _ => Err(value_error(
                xml_sdf,
                node,
                format!("The duration '{duration_str}' must be in either 'ticks' or 'us'"),
            )),
        }?;

        Ok(DomainSchedEntry { domain, duration })
    }

    pub fn has_domains(&self) -> bool {
        !self.name_to_id_map.is_empty()
    }
}

struct XmlSystemDescription<'a> {
    filename: &'a Path,
    doc: &'a roxmltree::Document<'a>,
}

#[derive(Debug)]
pub struct SystemDescription {
    pub protection_domains: Vec<ProtectionDomain>,
    pub memory_regions: Vec<SysMemoryRegion>,
    pub iomaps: Vec<SysIOMap>,
    pub channels: Vec<Channel>,
    pub domains: Domains,
}

fn location_suffix_format(xml_sdf: &XmlSystemDescription, text_pos: Option<SdfLocation>) -> String {
    text_pos
        .map(|pos| format!("@ {}", loc_string(xml_sdf, pos)))
        .unwrap_or_default()
}

// max_end is the first invalid virtual address
fn check_maps<'a, M, I>(
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

fn check_io_maps(
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

fn check_attributes(
    xml_sdf: &XmlSystemDescription,
    node: &dyn SdfNode,
    attributes: &[&'static str],
) -> Result<(), String> {
    for attribute in node.attributes() {
        if !attributes.contains(&attribute.name) {
            return Err(value_error(
                xml_sdf,
                node,
                format!("invalid attribute '{}'", attribute.name),
            ));
        }
    }

    Ok(())
}

fn checked_lookup<'a>(
    xml_sdf: &XmlSystemDescription,
    node: &'a dyn SdfNode,
    attribute: &'static str,
) -> Result<&'a str, String> {
    if let Some(value) = node.attribute(attribute) {
        Ok(value)
    } else {
        let pos = node.range().start;
        Err(format!(
            "Error: Missing required attribute '{}' on element '{}': {}:{}:{}",
            attribute,
            node.tag_name(),
            xml_sdf.filename.display(),
            pos.row,
            pos.col
        ))
    }
}

fn value_error(xml_sdf: &XmlSystemDescription, node: &dyn SdfNode, err: String) -> String {
    let pos = node.range().start;
    format!(
        "Error: {} on element '{}': {}:{}:{}",
        err,
        node.tag_name(),
        xml_sdf.filename.display(),
        pos.row,
        pos.col
    )
}

fn check_no_text(xml_sdf: &XmlSystemDescription, node: &roxmltree::Node) -> Result<(), String> {
    let name = node.tag_name().name();
    let pos = xml_sdf.doc.text_pos_at(node.range().start);
    let pos = SdfLocation {
        row: pos.row,
        col: pos.col,
    };

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

pub fn parse(
    filename: &Path,
    xml: &str,
    config: &Config,
    search_paths: &Vec<PathBuf>,
) -> Result<SystemDescription, String> {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(doc) => doc,
        Err(err) => return Err(format!("Could not parse '{0}': {err}", filename.display())),
    };

    let xml_sdf = XmlSystemDescription {
        filename,
        doc: &doc,
    };

    let mut root_pds = vec![];
    let mut mrs = vec![];
    let mut iomaps = vec![];
    let mut io_address_space_names = HashSet::new();
    let mut iommu_domain_ids = HashSet::new();
    let mut iommu_device_identifiers = Vec::new();
    let mut channels = vec![];
    let mut domains = Domains::default();
    let system = doc
        .root()
        .children()
        .find(|child| child.tag_name().name() == "system")
        .unwrap();

    // Ensure there is no non-whitespace/comment text
    check_no_text(&xml_sdf, &system)?;

    let system: &dyn SdfNode = &system;

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

    let mut pds = pd_flatten(&xml_sdf, root_pds)?;

    for node in channel_nodes {
        let ch = Channel::from_xml(&xml_sdf, &*node, &pds)?;

        if let Some(setvar_id) = &ch.end_a.setvar_id {
            let setvar = SysSetVar {
                symbol: setvar_id.to_string(),
                kind: SysSetVarKind::Id { id: ch.end_a.id },
            };
            checked_add_setvar(&mut pds[ch.end_a.pd].setvars, setvar, &xml_sdf, &*node)?;
        }

        if let Some(setvar_id) = &ch.end_b.setvar_id {
            let setvar = SysSetVar {
                symbol: setvar_id.to_string(),
                kind: SysSetVarKind::Id { id: ch.end_b.id },
            };
            checked_add_setvar(&mut pds[ch.end_b.pd].setvars, setvar, &xml_sdf, &*node)?;
        }

        channels.push(ch);
    }

    // FIXME: Now we post-fill the PD ids in the capmap elements, which is
    //        ugly, and we should rework this to be less so.
    let pd_names_to_id: HashMap<_, _> = pds
        .iter()
        .enumerate()
        .map(|(idx, pd)| (pd.name.clone(), idx))
        .collect();
    for cspace in pds.iter_mut().filter_map(|pd| pd.cspace.as_mut()) {
        for cap_map in cspace.cap_maps.iter_mut() {
            let Some(&pd) = pd_names_to_id.get(&cap_map.pd_name) else {
                return Err(format!(
                    "Error: unknown PD name '{}': {}",
                    cap_map.pd_name,
                    loc_string(&xml_sdf, cap_map.text_pos)
                ));
            };

            cap_map.pd = Some(pd);
        }
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
    for pd in &pds {
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
    let mut ch_ids = vec![vec![]; pds.len()];
    for (pd_idx, pd) in pds.iter().enumerate() {
        for sysirq in &pd.irqs {
            if ch_ids[pd_idx].contains(&sysirq.id) {
                return Err(format!(
                    "Error: duplicate channel id: {} in protection domain: '{}' @ {}:{}:{}",
                    sysirq.id,
                    pd.name,
                    filename.display(),
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
                filename.display(),
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
                filename.display(),
                pd.text_pos.unwrap().row,
                pd.text_pos.unwrap().col
            ));
        }

        let pd_a = &pds[ch.end_a.pd];
        let pd_b = &pds[ch.end_b.pd];
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
    for pd in &pds {
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
    for pd in &pds {
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
    for pd in &pds {
        let Some(cspace) = &pd.cspace else { continue };
        let mut user_cap_slots = HashMap::<u64, Vec<_>>::new();

        for cap_map in &cspace.cap_maps {
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
                        mapping.pd_name,
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
    for pd in pds.iter() {
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

fn checked_add_setvar(
    setvars: &mut Vec<SysSetVar>,
    setvar: SysSetVar,
    xml_sdf: &XmlSystemDescription<'_>,
    node: &dyn SdfNode<'_>,
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
