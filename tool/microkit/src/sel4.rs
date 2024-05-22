//
// Copyright 2024, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::io::{Write, BufWriter};
use std::collections::HashMap;
use crate::UntypedObject;

#[derive(Clone)]
pub struct BootInfo {
    pub fixed_cap_count: u64,
    pub sched_control_cap: u64,
    pub paging_cap_count: u64,
    pub page_cap_count: u64,
    pub untyped_objects: Vec<UntypedObject>,
    pub first_available_cap: u64,
}

/// Represents an allocated kernel object.
///
/// Kernel objects can have multiple caps (and caps can have multiple addresses).
/// The cap referred to here is the original cap that is allocated when the
/// kernel object is first allocate.
/// The cap_slot refers to the specific slot in which this cap resides.
/// The cap_address refers to a cap address that addresses this cap.
/// The cap_address is is intended to be valid within the context of the
/// initial task.
#[derive(Clone)]
pub struct Object {
    pub name: String,
    /// Type of kernel object
    pub object_type: ObjectType,
    pub cap_addr: u64,
    /// Physical memory address of the kernel object
    pub phys_addr: u64,
}

pub struct Config {
    pub arch: Arch,
    pub word_size: usize,
    pub minimum_page_size: u64,
    pub paddr_user_device_top: u64,
    pub kernel_frame_size: u64,
    pub init_cnode_bits: u64,
    pub cap_address_bits: u64,
    pub fan_out_limit: u64,
}

pub enum Arch {
    Aarch64,
}

#[repr(u64)]
#[derive(Debug, Hash, Eq, PartialEq, Copy, Clone)]
#[allow(dead_code)]
pub enum ObjectType {
    Untyped = 0,
    Tcb = 1,
    Endpoint = 2,
    Notification = 3,
    CNode = 4,
    SchedContext = 5,
    Reply = 6,
    HugePage = 7,
    VSpace = 8,
    SmallPage = 9,
    LargePage = 10,
    PageTable = 11,
}

impl ObjectType {
    pub fn fixed_size(&self) -> Option<u64> {
        match self {
            ObjectType::Tcb => Some(OBJECT_SIZE_TCB),
            ObjectType::Endpoint => Some(OBJECT_SIZE_ENDPOINT),
            ObjectType::Notification => Some(OBJECT_SIZE_NOTIFICATION),
            ObjectType::Reply => Some(OBJECT_SIZE_REPLY),
            ObjectType::VSpace => Some(OBJECT_SIZE_VSPACE),
            ObjectType::PageTable => Some(OBJECT_SIZE_PAGE_TABLE),
            ObjectType::LargePage => Some(OBJECT_SIZE_LARGE_PAGE),
            ObjectType::SmallPage => Some(OBJECT_SIZE_SMALL_PAGE),
            _ => None
        }
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            ObjectType::Untyped => "SEL4_UNTYPED_OBJECT",
            ObjectType::Tcb => "SEL4_TCB_OBJECT",
            ObjectType::Endpoint => "SEL4_ENDPOINT_OBJECT",
            ObjectType::Notification => "SEL4_NOTIFICATION_OBJECT",
            ObjectType::CNode => "SEL4_CNODE_OBJECT",
            ObjectType::SchedContext => "SEL4_SCHEDCONTEXT_OBJECT",
            ObjectType::Reply => "SEL4_REPLY_OBJECT",
            ObjectType::HugePage => "SEL4_HUGE_PAGE_OBJECT",
            ObjectType::VSpace => "SEL4_VSPACE_OBJECT",
            ObjectType::SmallPage => "SEL4_SMALL_PAGE_OBJECT",
            ObjectType::LargePage => "SEL4_LARGE_PAGE_OBJECT",
            ObjectType::PageTable => "SEL4_PAGE_TABLE_OBJECT",
        }
    }

    pub fn format(&self) -> String {
        let object_size = if let Some(fixed_size) = self.fixed_size() {
            format!("0x{:x}", fixed_size)
        } else {
            "variable size".to_string()
        };
        format!("         object_type          {} ({} - {})", *self as u64, self.to_str(), object_size)
    }
}

#[repr(u64)]
#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone)]
pub enum PageSize {
    Small = 0x1000,
    Large = 0x200_000,
}

impl From<u64> for PageSize {
    fn from(item: u64) -> PageSize {
        match item {
            0x1000 => PageSize::Small,
            0x200_000 => PageSize::Large,
            _ => panic!("Unknown page size {:x}", item),
        }
    }
}

pub const OBJECT_SIZE_TCB: u64 = 1 << 11;
pub const OBJECT_SIZE_ENDPOINT: u64 = 1 << 4;
pub const OBJECT_SIZE_NOTIFICATION: u64 = 1 << 6;
pub const OBJECT_SIZE_REPLY: u64 = 1 << 5;
pub const OBJECT_SIZE_PAGE_TABLE: u64 = 1 << 12;
pub const OBJECT_SIZE_LARGE_PAGE: u64 = 2 * 1024 * 1024;
pub const OBJECT_SIZE_SMALL_PAGE: u64 = 4 * 1024;
pub const OBJECT_SIZE_VSPACE: u64 = 4 * 1024;
// pub const OBJECT_SIZE_ASID_POOL: u64 = 1 << 12;

/// Virtual memory attributes for ARM
/// The values for each enum variant corresponds to what seL4
/// expects when doing a virtual memory invocation.
#[repr(u64)]
pub enum ArmVmAttributes {
    Cacheable = 1,
    ParityEnabled = 2,
    ExecuteNever = 4,
}

impl ArmVmAttributes {
    pub fn default() -> u64 {
        ArmVmAttributes::Cacheable as u64 | ArmVmAttributes::ParityEnabled as u64
    }
}

#[repr(u32)]
#[derive(Copy, Clone)]
#[allow(dead_code)]
pub enum Rights {
    None = 0x0,
    Write = 0x1,
    Read = 0x2,
    Grant = 0x4,
    GrantReply = 0x8,
    All = 0xf,
}

#[repr(u32)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum ArmIrqTrigger {
    Level = 0,
    Edge = 1,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum InvocationLabel {
    // Untyped
    UntypedRetype = 1,
    // TCB
    TcbReadRegisters = 2,
    TcbWriteRegisters = 3,
    TcbCopyRegisters = 4,
    TcbConfigure = 5,
    TcbSetPriority = 6,
    TcbSetMCPriority = 7,
    TcbSetSchedParams = 8,
    TcbSetTimeoutEndpoint = 9,
    TcbSetIpcBuffer = 10,
    TcbSetSpace = 11,
    TcbSuspend = 12,
    TcbResume = 13,
    TcbBindNotification = 14,
    TcbUnbindNotification = 15,
    TcbSetTLSBase = 16,
    // CNode
    CnodeRevoke = 17,
    CnodeDelete = 18,
    CnodeCancelBadgedSends = 19,
    CnodeCopy = 20,
    CnodeMint = 21,
    CnodeMove = 22,
    CnodeMutate = 23,
    CnodeRotate = 24,
    // IRQ
    IrqIssueIrqHandler = 25,
    IrqAckIrq = 26,
    IrqSetIrqHandler = 27,
    IrqClearIrqHandler = 28,
    // Domain
    DomainSetSet = 29,
    // Scheduling
    SchedControlConfigureFlags = 30,
    SchedContextBind = 31,
    SchedContextUnbind = 32,
    SchedContextUnbindObject = 33,
    SchedContextConsume = 34,
    SchedContextYieldTo = 35,
    // ARM VSpace
    ArmVspaceCleanData = 36,
    ArmVspaceInvalidateData = 37,
    ArmVspaceCleanInvalidateData = 38,
    ArmVspaceUnifyInstruction = 39,
    // ARM SMC
    ArmSmcCall = 40,
    // ARM Page table
    ArmPageTableMap = 41,
    ArmPageTableUnmap = 42,
    // ARM Page
    ArmPageMap = 43,
    ArmPageUnmap = 44,
    ArmPageCleanData = 45,
    ArmPageInvalidateData = 46,
    ArmPageCleanInvalidateData = 47,
    ArmPageUnifyInstruction = 48,
    ArmPageGetAddress = 49,
    // ARM Asid
    ArmAsidControlMakePool = 50,
    ArmAsidPoolAssign = 51,
    // ARM IRQ
    ArmIrqIssueIrqHandlerTrigger = 52,
}

#[derive(Copy, Clone)]
#[allow(dead_code)]
pub struct Aarch64Regs {
    pub pc: u64,
    pub sp: u64,
    pub spsr: u64,
    pub x0: u64,
    pub x1: u64,
    pub x2: u64,
    pub x3: u64,
    pub x4: u64,
    pub x5: u64,
    pub x6: u64,
    pub x7: u64,
    pub x8: u64,
    pub x16: u64,
    pub x17: u64,
    pub x18: u64,
    pub x29: u64,
    pub x30: u64,
    pub x9: u64,
    pub x10: u64,
    pub x11: u64,
    pub x12: u64,
    pub x13: u64,
    pub x14: u64,
    pub x15: u64,
    pub x19: u64,
    pub x20: u64,
    pub x21: u64,
    pub x22: u64,
    pub x23: u64,
    pub x24: u64,
    pub x25: u64,
    pub x26: u64,
    pub x27: u64,
    pub x28: u64,
    pub tpidr_el0: u64,
    pub tpidrro_el0: u64,
}

impl Aarch64Regs {
    // Returns a zero-initialised instance
    pub fn new() -> Aarch64Regs {
        Aarch64Regs {
            pc: 0,
            sp: 0,
            spsr: 0,
            x0: 0,
            x1: 0,
            x2: 0,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
            x8: 0,
            x16: 0,
            x17: 0,
            x18: 0,
            x29: 0,
            x30: 0,
            x9: 0,
            x10: 0,
            x11: 0,
            x12: 0,
            x13: 0,
            x14: 0,
            x15: 0,
            x19: 0,
            x20: 0,
            x21: 0,
            x22: 0,
            x23: 0,
            x24: 0,
            x25: 0,
            x26: 0,
            x27: 0,
            x28: 0,
            tpidr_el0: 0,
            tpidrro_el0: 0,
        }
    }

    pub fn field_names(&self) -> [(&'static str, u64); 36] {
        [
            ("pc", self.pc),
            ("sp", self.sp),
            ("spsr", self.spsr),
            ("x0", self.x0),
            ("x1", self.x1),
            ("x2", self.x2),
            ("x3", self.x3),
            ("x4", self.x4),
            ("x5", self.x5),
            ("x6", self.x6),
            ("x7", self.x7),
            ("x8", self.x8),
            ("x16", self.x16),
            ("x17", self.x17),
            ("x18", self.x18),
            ("x29", self.x29),
            ("x30", self.x30),
            ("x9", self.x9),
            ("x10", self.x10),
            ("x11", self.x11),
            ("x12", self.x12),
            ("x13", self.x13),
            ("x14", self.x14),
            ("x15", self.x15),
            ("x19", self.x19),
            ("x20", self.x20),
            ("x21", self.x21),
            ("x22", self.x22),
            ("x23", self.x23),
            ("x24", self.x24),
            ("x25", self.x25),
            ("x26", self.x26),
            ("x27", self.x27),
            ("x28", self.x28),
            ("tpidr_el0", self.tpidr_el0),
            ("tpidrro_el0", self.tpidrro_el0),
        ]
    }

    pub fn as_slice(&self) -> [u64; 36] {
        [
            self.pc,
            self.sp,
            self.spsr,
            self.x0,
            self.x1,
            self.x2,
            self.x3,
            self.x4,
            self.x5,
            self.x6,
            self.x7,
            self.x8,
            self.x16,
            self.x17,
            self.x18,
            self.x29,
            self.x30,
            self.x9,
            self.x10,
            self.x11,
            self.x12,
            self.x13,
            self.x14,
            self.x15,
            self.x19,
            self.x20,
            self.x21,
            self.x22,
            self.x23,
            self.x24,
            self.x25,
            self.x26,
            self.x27,
            self.x28,
            self.tpidr_el0,
            self.tpidrro_el0,
        ]
    }

    /// Just returns the count of registers
    pub const fn len(&self) -> u64 {
        36
    }
}

pub struct Invocation {
    label: InvocationLabel,
    args: InvocationArgs,
    repeat: Option<(u32, InvocationArgs)>,
}

impl Invocation {
    pub fn new(args: InvocationArgs) -> Invocation {
        Invocation {
            label: args.to_label(),
            args,
            repeat: None,
        }
    }

    /// Convert our higher-level representation of a seL4 invocation
    /// into raw bytes that will be given to the monitor to interpret
    /// at runtime.
    /// Appends to the given data
    pub fn add_raw_invocation(&self, data: &mut Vec<u8>) {
        let (service, args, extra_caps): (u64, Vec<u64>, Vec<u64>) = self.args.get_args();

        // To potentionally save some allocation, we reserve enough space for all the invocation args
        data.reserve(2 + args.len() * 8 + extra_caps.len() * 8);

        let mut tag = Invocation::message_info_new(self.label as u64, 0, extra_caps.len() as u64, args.len() as u64);
        if let Some((count, _)) = self.repeat {
            tag |= ((count - 1) as u64) << 32;
        }

        data.extend(tag.to_le_bytes());
        data.extend(service.to_le_bytes());
        for arg in extra_caps {
            data.extend(arg.to_le_bytes());
        }
        for arg in args {
            data.extend(arg.to_le_bytes());
        }

        if let Some((_, repeat)) = self.repeat {
            // Assert that the variant of the invocation arguments is the
            // same as the repeat invocation argument variant.
            assert!(std::mem::discriminant(&self.args) == std::mem::discriminant(&repeat));

            let (repeat_service, repeat_args, repeat_extra_caps) = repeat.get_args();
            data.extend(repeat_service.to_le_bytes());
            for cap in repeat_extra_caps {
                data.extend(cap.to_le_bytes());
            }
            for arg in repeat_args {
                data.extend(arg.to_le_bytes());
            }
        }
    }

    /// With how count is used when we convert the invocation, it is limited to a u32.
    pub fn repeat(&mut self, count: u32, repeat_args: InvocationArgs) {
        assert!(self.repeat.is_none());
        if count > 1 {
            self.repeat = Some((count, repeat_args));
        }
    }

    pub fn message_info_new(label: u64, caps: u64, extra_caps: u64, length: u64) -> u64 {
        assert!(label < (1 << 50));
        assert!(caps < 8);
        assert!(extra_caps < 8);
        assert!(length < 0x80);

        label << 12 | caps << 9 | extra_caps << 7 | length
    }

    fn fmt_field(field_name: &'static str, value: u64) -> String {
        format!("         {:<20} {}", field_name, value)
    }

    fn fmt_field_str(field_name: &'static str, value: String) -> String {
        format!("         {:<20} {}", field_name, value)
    }

    fn fmt_field_hex(field_name: &'static str, value: u64) -> String {
        format!("         {:<20} 0x{:x}", field_name, value)
    }

    fn fmt_field_reg(reg: &'static str, value: u64) -> String {
        format!("{}: 0x{:016x}", reg, value)
    }

    fn fmt_field_bool(field_name: &'static str, value: bool) -> String {
        format!("         {:<20} {}", field_name, value.to_string())
    }

    fn fmt_field_cap(field_name: &'static str, cap: u64, cap_lookup: &HashMap<u64, String>) -> String {
        let s = if let Some(name) = cap_lookup.get(&cap) {
            name
        } else {
            "None"
        };
        let field = format!("{} (cap)", field_name);
        format!("         {:<20} 0x{:016x} ({})", field, cap, s)
    }

    // This function is not particularly elegant. What is happening is that we are formatting
    // each invocation and its arguments depending on the kind of argument.
    // We do this in an explicit way due to there only being a dozen or so invocations rather
    // than involving some complicated macros, although maybe there is a better way I am not
    // aware of.
    pub fn report_fmt<W: Write>(&self, f: &mut BufWriter<W>, cap_lookup: &HashMap<u64, String>) {
        let mut arg_strs = Vec::new();
        let (service, service_str) = match self.args {
            InvocationArgs::UntypedRetype { untyped, object_type, size_bits, root, node_index, node_depth, node_offset, num_objects } => {
                arg_strs.push(object_type.format());
                let sz_fmt = if size_bits == 0 {
                    String::from("N/A")
                } else {
                    format!("0x{:x}", 1 << size_bits)
                };
                arg_strs.push(Invocation::fmt_field_str("size_bits", format!("{} ({})", size_bits, sz_fmt)));
                arg_strs.push(Invocation::fmt_field_cap("root", root, cap_lookup));
                arg_strs.push(Invocation::fmt_field("node_index", node_index));
                arg_strs.push(Invocation::fmt_field("node_depth", node_depth));
                arg_strs.push(Invocation::fmt_field("node_offset", node_offset));
                arg_strs.push(Invocation::fmt_field("num_objects", num_objects));
                (untyped, cap_lookup.get(&untyped).unwrap().as_str())
            }
            InvocationArgs::TcbSetSchedParams { tcb, authority, mcp, priority, sched_context, fault_ep } => {
                arg_strs.push(Invocation::fmt_field_cap("authority", authority, cap_lookup));
                arg_strs.push(Invocation::fmt_field("mcp", mcp));
                arg_strs.push(Invocation::fmt_field("priority", priority));
                arg_strs.push(Invocation::fmt_field_cap("sched_context", sched_context, cap_lookup));
                arg_strs.push(Invocation::fmt_field_cap("fault_ep", fault_ep, cap_lookup));
                (tcb, cap_lookup.get(&tcb).unwrap().as_str())
            }
            InvocationArgs::TcbSetSpace { tcb, fault_ep, cspace_root, cspace_root_data, vspace_root, vspace_root_data } => {
                arg_strs.push(Invocation::fmt_field_cap("fault_ep", fault_ep, cap_lookup));
                arg_strs.push(Invocation::fmt_field_cap("cspace_root", cspace_root, cap_lookup));
                arg_strs.push(Invocation::fmt_field("cspace_root_data", cspace_root_data));
                arg_strs.push(Invocation::fmt_field_cap("vspace_root", vspace_root, cap_lookup));
                arg_strs.push(Invocation::fmt_field("vspace_root_data", vspace_root_data));
                (tcb, cap_lookup.get(&tcb).unwrap().as_str())
            }
            InvocationArgs::TcbSetIpcBuffer { tcb, buffer, buffer_frame } => {
                arg_strs.push(Invocation::fmt_field_hex("buffer", buffer));
                arg_strs.push(Invocation::fmt_field_cap("buffer_frame", buffer_frame, cap_lookup));
                (tcb, cap_lookup.get(&tcb).unwrap().as_str())
            }
            InvocationArgs::TcbResume { tcb } => {
                (tcb, cap_lookup.get(&tcb).unwrap().as_str())
            }
            InvocationArgs::TcbWriteRegisters { tcb, resume, arch_flags, regs, .. } => {
                arg_strs.push(Invocation::fmt_field_bool("resume", resume));
                arg_strs.push(Invocation::fmt_field("arch_flags", arch_flags as u64));

                let reg_strs = regs.field_names()
                                   .into_iter()
                                   .map(|(field, val)| Invocation::fmt_field_reg(field, val))
                                   .collect::<Vec<_>>();
                arg_strs.push(Invocation::fmt_field_str("regs", reg_strs[0].clone()));
                for s in &reg_strs[1..] {
                    arg_strs.push(format!("                              {}", s));
                }

                (tcb, cap_lookup.get(&tcb).unwrap().as_str())
            }
            InvocationArgs::TcbBindNotification { tcb, notification } => {
                arg_strs.push(Invocation::fmt_field_cap("notification", notification, cap_lookup));
                (tcb, cap_lookup.get(&tcb).unwrap().as_str())
            }
            InvocationArgs::AsidPoolAssign { asid_pool, vspace } => {
                arg_strs.push(Invocation::fmt_field_cap("vspace", vspace, cap_lookup));
                (asid_pool, cap_lookup.get(&asid_pool).unwrap().as_str())
            }
            InvocationArgs::IrqControlGetTrigger { irq_control, irq, trigger, dest_root, dest_index, dest_depth } => {
                arg_strs.push(Invocation::fmt_field("irq", irq));
                arg_strs.push(Invocation::fmt_field("trigger", trigger as u64));
                arg_strs.push(Invocation::fmt_field_cap("dest_root", dest_root, cap_lookup));
                arg_strs.push(Invocation::fmt_field("dest_index", dest_index));
                arg_strs.push(Invocation::fmt_field("dest_depth", dest_depth));
                (irq_control, cap_lookup.get(&irq_control).unwrap().as_str())
            }
            InvocationArgs::IrqHandlerSetNotification { irq_handler, notification } => {
                arg_strs.push(Invocation::fmt_field_cap("notification", notification, cap_lookup));
                (irq_handler, cap_lookup.get(&irq_handler).unwrap().as_str())
            }
            InvocationArgs::PageTableMap { page_table, vspace, vaddr, attr } => {
                arg_strs.push(Invocation::fmt_field_cap("vspace", vspace, cap_lookup));
                arg_strs.push(Invocation::fmt_field_hex("vaddr", vaddr));
                arg_strs.push(Invocation::fmt_field("attr", attr));
                (page_table, cap_lookup.get(&page_table).unwrap().as_str())
            }
            InvocationArgs::PageMap { page, vspace, vaddr, rights, attr } => {
                arg_strs.push(Invocation::fmt_field_cap("vspace", vspace, cap_lookup));
                arg_strs.push(Invocation::fmt_field_hex("vaddr", vaddr));
                arg_strs.push(Invocation::fmt_field("rights", rights));
                arg_strs.push(Invocation::fmt_field("attr", attr));
                (page, cap_lookup.get(&page).unwrap().as_str())
            }
            InvocationArgs::CnodeMint { cnode, dest_index, dest_depth, src_root, src_obj, src_depth, rights, badge } => {
                arg_strs.push(Invocation::fmt_field("dest_index", dest_index));
                arg_strs.push(Invocation::fmt_field("dest_depth", dest_depth));
                arg_strs.push(Invocation::fmt_field_cap("src_root", src_root, cap_lookup));
                arg_strs.push(Invocation::fmt_field_cap("src_obj", src_obj, cap_lookup));
                arg_strs.push(Invocation::fmt_field("src_depth", src_depth));
                arg_strs.push(Invocation::fmt_field("rights", rights));
                arg_strs.push(Invocation::fmt_field("badge", badge));
                (cnode, cap_lookup.get(&cnode).unwrap().as_str())
            }
            InvocationArgs::SchedControlConfigureFlags { sched_control, sched_context, budget, period, extra_refills, badge, flags } => {
                arg_strs.push(Invocation::fmt_field_cap("schedcontext", sched_context, cap_lookup));
                arg_strs.push(Invocation::fmt_field("budget", budget));
                arg_strs.push(Invocation::fmt_field("period", period));
                arg_strs.push(Invocation::fmt_field("extra_refills", extra_refills));
                arg_strs.push(Invocation::fmt_field("badge", badge));
                arg_strs.push(Invocation::fmt_field("flags", flags));
                (sched_control, "None")
            }
        };
        _ = writeln!(f, "{:<20} - {:<17} - 0x{:016x} ({})\n{}", self.object_type(), self.method_name(), service, service_str, arg_strs.join("\n"));
        if let Some((count, _)) = self.repeat {
            _ = writeln!(f, "      REPEAT: count={}", count);
        }
    }

    fn object_type(&self) -> &'static str {
        match self.label {
            InvocationLabel::UntypedRetype => "Untyped",
            InvocationLabel::TcbSetSchedParams |
            InvocationLabel::TcbSetSpace |
            InvocationLabel::TcbSetIpcBuffer |
            InvocationLabel::TcbResume |
            InvocationLabel::TcbWriteRegisters |
            InvocationLabel::TcbBindNotification => "TCB",
            InvocationLabel::ArmAsidPoolAssign => "ASID Pool",
            InvocationLabel::ArmIrqIssueIrqHandlerTrigger => "IRQ Control",
            InvocationLabel::IrqSetIrqHandler => "IRQ Handler",
            InvocationLabel::ArmPageTableMap => "Page Table",
            InvocationLabel::ArmPageMap => "Page",
            InvocationLabel::CnodeMint => "CNode",
            InvocationLabel::SchedControlConfigureFlags => "SchedControl",
            _ => panic!("Internal error: unexpected label when getting object type '{:?}'", self.label)
        }
    }

    fn method_name(&self) -> &'static str {
        match self.label {
            InvocationLabel::UntypedRetype => "Retype",
            InvocationLabel::TcbSetSchedParams => "SetSchedParams",
            InvocationLabel::TcbSetSpace => "SetSpace",
            InvocationLabel::TcbSetIpcBuffer => "SetIPCBuffer",
            InvocationLabel::TcbResume => "Resume",
            InvocationLabel::TcbWriteRegisters => "WriteRegisters",
            InvocationLabel::TcbBindNotification => "BindNotification",
            InvocationLabel::ArmAsidPoolAssign => "Assign",
            InvocationLabel::ArmIrqIssueIrqHandlerTrigger => "Get",
            InvocationLabel::IrqSetIrqHandler => "SetNotification",
            InvocationLabel::ArmPageTableMap |
            InvocationLabel::ArmPageMap => "Map",
            InvocationLabel::CnodeMint => "Mint",
            InvocationLabel::SchedControlConfigureFlags => "ConfigureFlags",
            _ => panic!("Internal error: unexpected label when getting method name '{:?}'", self.label)
        }
    }
}

impl InvocationArgs {
    fn to_label(self) -> InvocationLabel {
        match self {
            InvocationArgs::UntypedRetype { .. } => InvocationLabel::UntypedRetype,
            InvocationArgs::TcbSetSchedParams { .. } => InvocationLabel::TcbSetSchedParams,
            InvocationArgs::TcbSetSpace { .. } => InvocationLabel::TcbSetSpace,
            InvocationArgs::TcbSetIpcBuffer { .. } => InvocationLabel::TcbSetIpcBuffer,
            InvocationArgs::TcbResume { .. } => InvocationLabel::TcbResume,
            InvocationArgs::TcbWriteRegisters { .. } => InvocationLabel::TcbWriteRegisters,
            InvocationArgs::TcbBindNotification { .. } => InvocationLabel::TcbBindNotification,
            InvocationArgs::AsidPoolAssign { .. } => InvocationLabel::ArmAsidPoolAssign,
            InvocationArgs::IrqControlGetTrigger { .. } => InvocationLabel::ArmIrqIssueIrqHandlerTrigger,
            InvocationArgs::IrqHandlerSetNotification { .. } => InvocationLabel::IrqSetIrqHandler,
            InvocationArgs::PageTableMap { .. } => InvocationLabel::ArmPageTableMap,
            InvocationArgs::PageMap { .. } => InvocationLabel::ArmPageMap,
            InvocationArgs::CnodeMint { .. } => InvocationLabel::CnodeMint,
            InvocationArgs::SchedControlConfigureFlags { .. } => InvocationLabel::SchedControlConfigureFlags,
        }
    }

    fn get_args(self) -> (u64, Vec<u64>, Vec<u64>) {
        match self {
            InvocationArgs::UntypedRetype { untyped, object_type, size_bits, root, node_index, node_depth, node_offset, num_objects } =>
                                        (
                                           untyped,
                                           vec![object_type as u64, size_bits, node_index, node_depth, node_offset, num_objects],
                                           vec![root]
                                        ),
            InvocationArgs::TcbSetSchedParams { tcb, authority, mcp, priority, sched_context, fault_ep } =>
                                        (
                                            tcb,
                                            vec![mcp, priority],
                                            vec![authority, sched_context, fault_ep]
                                        ),
            InvocationArgs::TcbSetSpace { tcb, fault_ep, cspace_root, cspace_root_data, vspace_root, vspace_root_data } =>
                                        (
                                            tcb,
                                            vec![cspace_root_data, vspace_root_data],
                                            vec![fault_ep, cspace_root, vspace_root]
                                        ),
            InvocationArgs::TcbSetIpcBuffer { tcb, buffer, buffer_frame } => (tcb, vec![buffer], vec![buffer_frame]),
            InvocationArgs::TcbResume { tcb } => (tcb, vec![], vec![]),
            InvocationArgs::TcbWriteRegisters { tcb, resume, arch_flags, regs, count } => {
                // Here there are a couple of things going on.
                // The invocation arguments to do not correspond one-to-one to word size,
                // so we have to do some packing first.
                // This means that the resume and arch_flags arguments need to be packed into
                // a single word. We then add all the registers which are each the size of a word.
                let resume_byte = if resume { 1 } else { 0 };
                let flags: u64 = ((arch_flags as u64) << 8) | resume_byte;
                let mut args = vec![flags, count];
                args.extend(regs.as_slice());
                (tcb, args, vec![])
            }
            InvocationArgs::TcbBindNotification { tcb, notification } => (tcb, vec![], vec![notification]),
            InvocationArgs::AsidPoolAssign { asid_pool, vspace } => (asid_pool, vec![], vec![vspace]),
            InvocationArgs::IrqControlGetTrigger { irq_control, irq, trigger, dest_root, dest_index, dest_depth } =>
                                        (
                                            irq_control,
                                            vec![irq, trigger as u64, dest_index, dest_depth],
                                            vec![dest_root],
                                        ),
            InvocationArgs::IrqHandlerSetNotification { irq_handler, notification } => (irq_handler, vec![], vec![notification]),
            InvocationArgs::PageTableMap { page_table, vspace, vaddr, attr } =>
                                        (
                                            page_table,
                                            vec![vaddr, attr],
                                            vec![vspace]
                                        ),
            InvocationArgs::PageMap { page, vspace, vaddr, rights, attr } => (page, vec![vaddr, rights as u64, attr], vec![vspace]),
            InvocationArgs::CnodeMint { cnode, dest_index, dest_depth, src_root, src_obj, src_depth, rights, badge } =>
                                        (
                                            cnode,
                                            vec![dest_index, dest_depth, src_obj, src_depth, rights as u64, badge],
                                            vec![src_root]
                                        ),
            InvocationArgs::SchedControlConfigureFlags { sched_control, sched_context, budget, period, extra_refills, badge, flags } =>
                                        (
                                            sched_control,
                                            vec![budget, period, extra_refills, badge, flags],
                                            vec![sched_context]
                                        )
        }
    }
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum InvocationArgs {
    UntypedRetype {
        untyped: u64,
        object_type: ObjectType,
        size_bits: u64,
        root: u64,
        node_index: u64,
        node_depth: u64,
        node_offset: u64,
        num_objects: u64
    },
    TcbSetSchedParams {
        tcb: u64,
        authority: u64,
        mcp: u64,
        priority: u64,
        sched_context: u64,
        fault_ep: u64,
    },
    TcbSetSpace {
        tcb: u64,
        fault_ep: u64,
        cspace_root: u64,
        cspace_root_data: u64,
        vspace_root: u64,
        vspace_root_data: u64,
    },
    TcbSetIpcBuffer {
        tcb: u64,
        buffer: u64,
        buffer_frame: u64,
    },
    TcbResume {
        tcb: u64,
    },
    TcbWriteRegisters {
        tcb: u64,
        resume: bool,
        arch_flags: u8,
        count: u64,
        regs: Aarch64Regs,
    },
    TcbBindNotification {
        tcb: u64,
        notification: u64,
    },
    AsidPoolAssign {
        asid_pool: u64,
        vspace: u64,
    },
    IrqControlGetTrigger {
        irq_control: u64,
        irq: u64,
        trigger: ArmIrqTrigger,
        dest_root: u64,
        dest_index: u64,
        dest_depth: u64,
    },
    IrqHandlerSetNotification {
        irq_handler: u64,
        notification: u64,
    },
    PageTableMap {
        page_table: u64,
        vspace: u64,
        vaddr: u64,
        attr: u64,
    },
    PageMap {
        page: u64,
        vspace: u64,
        vaddr: u64,
        rights: u64,
        attr: u64,
    },
    CnodeMint {
        cnode: u64,
        dest_index: u64,
        dest_depth: u64,
        src_root: u64,
        src_obj: u64,
        src_depth: u64,
        rights: u64,
        badge: u64,
    },
    SchedControlConfigureFlags {
        sched_control: u64,
        sched_context: u64,
        budget: u64,
        period: u64,
        extra_refills: u64,
        badge: u64,
        flags: u64,
    }
}
