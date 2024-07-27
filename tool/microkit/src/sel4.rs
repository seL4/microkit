//
// Copyright 2024, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use crate::UntypedObject;
use std::collections::HashMap;
use std::io::{BufWriter, Write};

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
    /// Type of kernel object
    pub object_type: ObjectType,
    pub cap_addr: u64,
    /// Physical memory address of the kernel object
    pub phys_addr: u64,
}

pub struct Config {
    pub arch: Arch,
    pub word_size: u64,
    pub minimum_page_size: u64,
    pub paddr_user_device_top: u64,
    pub kernel_frame_size: u64,
    pub init_cnode_bits: u64,
    pub cap_address_bits: u64,
    pub fan_out_limit: u64,
    pub hypervisor: bool,
    pub benchmark: bool,
    pub fpu: bool,
    /// ARM-specific, number of physical address bits
    pub arm_pa_size_bits: Option<usize>,
    /// RISC-V specific, what kind of virtual memory system (e.g Sv39)
    pub riscv_pt_levels: Option<RiscvVirtualMemory>,
}

impl Config {
    pub fn user_top(&self) -> u64 {
        match self.arch {
            Arch::Aarch64 => match self.hypervisor {
                true => match self.arm_pa_size_bits.unwrap() {
                    40 => 0x10000000000,
                    44 => 0x100000000000,
                    _ => panic!("Unknown ARM physical address size bits"),
                },
                false => 0x800000000000,
            },
            Arch::Riscv64 => 0x0000003ffffff000,
        }
    }
}

pub enum Arch {
    Aarch64,
    Riscv64,
}

/// RISC-V supports multiple virtual memory systems and so we use this enum
/// to make it easier to support more virtual memory systems in the future.
#[derive(Debug, Copy, Clone)]
pub enum RiscvVirtualMemory {
    Sv39,
}

impl RiscvVirtualMemory {
    /// Returns number of page-table levels for a particular virtual memory system.
    pub fn levels(self) -> usize {
        match self {
            RiscvVirtualMemory::Sv39 => 3,
        }
    }
}

#[derive(Debug, Hash, Eq, PartialEq, Copy, Clone)]
pub enum ObjectType {
    Untyped,
    Tcb,
    Endpoint,
    Notification,
    CNode,
    SchedContext,
    Reply,
    HugePage,
    VSpace,
    SmallPage,
    LargePage,
    PageTable,
    Vcpu,
}

impl ObjectType {
    /// Gets the number of bits to represent the size of a object. The
    /// size depends on architecture as well as kernel configuration.
    pub fn fixed_size_bits(self, config: &Config) -> Option<u64> {
        match self {
            ObjectType::Tcb => match config.arch {
                Arch::Aarch64 => Some(11),
                Arch::Riscv64 => match config.fpu {
                    true => Some(11),
                    false => Some(10),
                },
            },
            ObjectType::Endpoint => Some(4),
            ObjectType::Notification => Some(6),
            ObjectType::Reply => Some(5),
            ObjectType::VSpace => match config.arch {
                Arch::Aarch64 => match config.hypervisor {
                    true => match config.arm_pa_size_bits.unwrap() {
                        40 => Some(13),
                        44 => Some(12),
                        _ => {
                            panic!("Unexpected ARM PA size bits when determining VSpace size bits")
                        }
                    },
                    false => Some(12),
                },
                Arch::Riscv64 => Some(12),
            },
            ObjectType::PageTable => Some(12),
            ObjectType::HugePage => Some(30),
            ObjectType::LargePage => Some(21),
            ObjectType::SmallPage => Some(12),
            ObjectType::Vcpu => match config.arch {
                Arch::Aarch64 => Some(12),
                _ => panic!("Unexpected architecture asking for vCPU size bits"),
            },
            _ => None,
        }
    }

    pub fn fixed_size(self, config: &Config) -> Option<u64> {
        self.fixed_size_bits(config).map(|bits| 1 << bits)
    }

    pub fn to_str(self) -> &'static str {
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
            ObjectType::Vcpu => "SEL4_VCPU_OBJECT",
        }
    }

    /// The kernel associates each kernel object with an identifier, which
    /// also depends on the configuration of the kernel.
    /// When generating the raw invocation to be given to the initial task,
    /// this method must be called for any UntypedRetype invocations.
    pub fn value(self, config: &Config) -> u64 {
        match self {
            ObjectType::Untyped => 0,
            ObjectType::Tcb => 1,
            ObjectType::Endpoint => 2,
            ObjectType::Notification => 3,
            ObjectType::CNode => 4,
            ObjectType::SchedContext => 5,
            ObjectType::Reply => 6,
            ObjectType::HugePage => 7,
            ObjectType::VSpace => match config.arch {
                Arch::Aarch64 => 8,
                Arch::Riscv64 => 10,
            },
            ObjectType::SmallPage => match config.arch {
                Arch::Aarch64 => 9,
                Arch::Riscv64 => 8,
            },
            ObjectType::LargePage => match config.arch {
                Arch::Aarch64 => 10,
                Arch::Riscv64 => 9,
            },
            ObjectType::PageTable => match config.arch {
                Arch::Aarch64 => 11,
                Arch::Riscv64 => 10,
            },
            ObjectType::Vcpu => match config.arch {
                Arch::Aarch64 => 12,
                _ => panic!("Unknown vCPU object type value for given kernel config"),
            },
        }
    }

    pub fn format(&self, config: &Config) -> String {
        let object_size = if let Some(fixed_size) = self.fixed_size(config) {
            format!("0x{:x}", fixed_size)
        } else {
            "variable size".to_string()
        };
        format!(
            "         object_type          {} ({} - {})",
            self.value(config),
            self.to_str(),
            object_size
        )
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

/// Virtual memory attributes for ARM
/// The values for each enum variant corresponds to what seL4
/// expects when doing a virtual memory invocation.
#[repr(u64)]
pub enum ArmVmAttributes {
    Cacheable = 1,
    ParityEnabled = 2,
    ExecuteNever = 4,
}

/// Virtual memory attributes for RISC-V
/// The values for each enum variant corresponds to what seL4
/// expects when doing a virtual memory invocation.
#[repr(u64)]
pub enum RiscvVmAttributes {
    ExecuteNever = 1,
}

impl ArmVmAttributes {
    #[allow(clippy::should_implement_trait)] // Default::default would return Self, not u64
    pub fn default() -> u64 {
        ArmVmAttributes::Cacheable as u64 | ArmVmAttributes::ParityEnabled as u64
    }
}

impl RiscvVmAttributes {
    #[allow(clippy::should_implement_trait)] // Default::default would return Self, not u64
    pub fn default() -> u64 {
        0
    }
}

pub fn default_vm_attr(config: &Config) -> u64 {
    match config.arch {
        Arch::Aarch64 => ArmVmAttributes::default(),
        Arch::Riscv64 => RiscvVmAttributes::default(),
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
/// The same values apply to all kernel architectures
pub enum IrqTrigger {
    Level = 0,
    Edge = 1,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum InvocationLabel {
    // Untyped
    UntypedRetype,
    // TCB
    TcbReadRegisters,
    TcbWriteRegisters,
    TcbCopyRegisters,
    TcbConfigure,
    TcbSetPriority,
    TcbSetMCPriority,
    TcbSetSchedParams,
    TcbSetTimeoutEndpoint,
    TcbSetIpcBuffer,
    TcbSetSpace,
    TcbSuspend,
    TcbResume,
    TcbBindNotification,
    TcbUnbindNotification,
    TcbSetTLSBase,
    // CNode
    CnodeRevoke,
    CnodeDelete,
    CnodeCancelBadgedSends,
    CnodeCopy,
    CnodeMint,
    CnodeMove,
    CnodeMutate,
    CnodeRotate,
    // IRQ
    IrqIssueIrqHandler,
    IrqAckIrq,
    IrqSetIrqHandler,
    IrqClearIrqHandler,
    // Domain
    DomainSetSet,
    // Scheduling
    SchedControlConfigureFlags,
    SchedContextBind,
    SchedContextUnbind,
    SchedContextUnbindObject,
    SchedContextConsume,
    SchedContextYieldTo,
    // ARM VSpace
    ArmVspaceCleanData,
    ArmVspaceInvalidateData,
    ArmVspaceCleanInvalidateData,
    ArmVspaceUnifyInstruction,
    // ARM SMC
    ArmSmcCall,
    // ARM Page table
    ArmPageTableMap,
    ArmPageTableUnmap,
    // ARM Page
    ArmPageMap,
    ArmPageUnmap,
    ArmPageCleanData,
    ArmPageInvalidateData,
    ArmPageCleanInvalidateData,
    ArmPageUnifyInstruction,
    ArmPageGetAddress,
    // ARM Asid
    ArmAsidControlMakePool,
    ArmAsidPoolAssign,
    // ARM vCPU
    ArmVcpuSetTcb,
    ArmVcpuInjectIrq,
    ArmVcpuReadReg,
    ArmVcpuWriteReg,
    ArmVcpuAckVppi,
    // ARM IRQ
    ArmIrqIssueIrqHandlerTrigger,
    // RISC-V Page Table
    RiscvPageTableMap,
    RiscvPageTableUnmap,
    // RISC-V Page
    RiscvPageMap,
    RiscvPageUnmap,
    RiscvPageGetAddress,
    // RISC-V ASID
    RiscvAsidControlMakePool,
    RiscvAsidPoolAssign,
    // RISC-V IRQ
    RiscvIrqIssueIrqHandlerTrigger,
}

impl InvocationLabel {
    /// Convert an invocation's named label to the value seL4 expects when
    /// you make the invocation.
    pub fn to_value(self) -> u32 {
        match self {
            InvocationLabel::UntypedRetype => 1,
            InvocationLabel::TcbReadRegisters => 2,
            InvocationLabel::TcbWriteRegisters => 3,
            InvocationLabel::TcbCopyRegisters => 4,
            InvocationLabel::TcbConfigure => 5,
            InvocationLabel::TcbSetPriority => 6,
            InvocationLabel::TcbSetMCPriority => 7,
            InvocationLabel::TcbSetSchedParams => 8,
            InvocationLabel::TcbSetTimeoutEndpoint => 9,
            InvocationLabel::TcbSetIpcBuffer => 10,
            InvocationLabel::TcbSetSpace => 11,
            InvocationLabel::TcbSuspend => 12,
            InvocationLabel::TcbResume => 13,
            InvocationLabel::TcbBindNotification => 14,
            InvocationLabel::TcbUnbindNotification => 15,
            InvocationLabel::TcbSetTLSBase => 16,
            // CNode
            InvocationLabel::CnodeRevoke => 17,
            InvocationLabel::CnodeDelete => 18,
            InvocationLabel::CnodeCancelBadgedSends => 19,
            InvocationLabel::CnodeCopy => 20,
            InvocationLabel::CnodeMint => 21,
            InvocationLabel::CnodeMove => 22,
            InvocationLabel::CnodeMutate => 23,
            InvocationLabel::CnodeRotate => 24,
            // IRQ
            InvocationLabel::IrqIssueIrqHandler => 25,
            InvocationLabel::IrqAckIrq => 26,
            InvocationLabel::IrqSetIrqHandler => 27,
            InvocationLabel::IrqClearIrqHandler => 28,
            // Domain
            InvocationLabel::DomainSetSet => 29,
            // Scheduling
            InvocationLabel::SchedControlConfigureFlags => 30,
            InvocationLabel::SchedContextBind => 31,
            InvocationLabel::SchedContextUnbind => 32,
            InvocationLabel::SchedContextUnbindObject => 33,
            InvocationLabel::SchedContextConsume => 34,
            InvocationLabel::SchedContextYieldTo => 35,
            // ARM VSpace
            InvocationLabel::ArmVspaceCleanData => 36,
            InvocationLabel::ArmVspaceInvalidateData => 37,
            InvocationLabel::ArmVspaceCleanInvalidateData => 38,
            InvocationLabel::ArmVspaceUnifyInstruction => 39,
            // ARM SMC
            InvocationLabel::ArmSmcCall => 40,
            // ARM Page table
            InvocationLabel::ArmPageTableMap => 41,
            InvocationLabel::ArmPageTableUnmap => 42,
            // ARM Page
            InvocationLabel::ArmPageMap => 43,
            InvocationLabel::ArmPageUnmap => 44,
            InvocationLabel::ArmPageCleanData => 45,
            InvocationLabel::ArmPageInvalidateData => 46,
            InvocationLabel::ArmPageCleanInvalidateData => 47,
            InvocationLabel::ArmPageUnifyInstruction => 48,
            InvocationLabel::ArmPageGetAddress => 49,
            // ARM ASID
            InvocationLabel::ArmAsidControlMakePool => 50,
            InvocationLabel::ArmAsidPoolAssign => 51,
            // ARM vCPU
            InvocationLabel::ArmVcpuSetTcb => 52,
            InvocationLabel::ArmVcpuInjectIrq => 53,
            InvocationLabel::ArmVcpuReadReg => 54,
            InvocationLabel::ArmVcpuWriteReg => 55,
            InvocationLabel::ArmVcpuAckVppi => 56,
            // ARM IRQ
            InvocationLabel::ArmIrqIssueIrqHandlerTrigger => 57,
            // RISC-V Page
            InvocationLabel::RiscvPageTableMap => 36,
            InvocationLabel::RiscvPageTableUnmap => 37,
            InvocationLabel::RiscvPageMap => 38,
            InvocationLabel::RiscvPageUnmap => 39,
            InvocationLabel::RiscvPageGetAddress => 40,
            // RISC-V ASID
            InvocationLabel::RiscvAsidControlMakePool => 41,
            InvocationLabel::RiscvAsidPoolAssign => 42,
            // RISC-V IRQ
            InvocationLabel::RiscvIrqIssueIrqHandlerTrigger => 43,
        }
    }
}

#[derive(Copy, Clone, Default)]
#[allow(dead_code)]
pub struct Riscv64Regs {
    pub pc: u64,
    pub ra: u64,
    pub sp: u64,
    pub gp: u64,
    pub s0: u64,
    pub s1: u64,
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub s9: u64,
    pub s10: u64,
    pub s11: u64,
    pub a0: u64,
    pub a1: u64,
    pub a2: u64,
    pub a3: u64,
    pub a4: u64,
    pub a5: u64,
    pub a6: u64,
    pub a7: u64,
    pub t0: u64,
    pub t1: u64,
    pub t2: u64,
    pub t3: u64,
    pub t4: u64,
    pub t5: u64,
    pub t6: u64,
    pub tp: u64,
}

impl Riscv64Regs {
    pub fn field_names(&self) -> Vec<(&'static str, u64)> {
        vec![
            ("pc", self.pc),
            ("ra", self.ra),
            ("sp", self.sp),
            ("gp", self.gp),
            ("s0", self.s0),
            ("s1", self.s1),
            ("s2", self.s2),
            ("s3", self.s3),
            ("s4", self.s4),
            ("s5", self.s5),
            ("s6", self.s6),
            ("s7", self.s7),
            ("s8", self.s8),
            ("s9", self.s9),
            ("s10", self.s10),
            ("s11", self.s11),
            ("a0", self.a0),
            ("a1", self.a1),
            ("a2", self.a2),
            ("a3", self.a3),
            ("a4", self.a4),
            ("a5", self.a5),
            ("a6", self.a6),
            ("a7", self.a7),
            ("t0", self.t0),
            ("t1", self.t1),
            ("t2", self.t2),
            ("t3", self.t3),
            ("t4", self.t4),
            ("t5", self.t5),
            ("t6", self.t6),
            ("tp", self.tp),
        ]
    }

    pub fn as_slice(&self) -> Vec<u64> {
        vec![
            self.pc, self.ra, self.sp, self.gp, self.s0, self.s1, self.s2, self.s3, self.s4,
            self.s5, self.s6, self.s7, self.s8, self.s9, self.s10, self.s11, self.a0, self.a1,
            self.a2, self.a3, self.a4, self.a5, self.a6, self.a7, self.t0, self.t1, self.t2,
            self.t3, self.t4, self.t5, self.t6, self.tp,
        ]
    }

    /// Number of registers
    pub const LEN: usize = 32;
}

#[derive(Copy, Clone, Default)]
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
    pub fn field_names(&self) -> Vec<(&'static str, u64)> {
        vec![
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

    pub fn as_slice(&self) -> Vec<u64> {
        vec![
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

    /// Number of registers
    pub const LEN: usize = 36;
}

pub struct Invocation {
    /// There is some careful context to be aware of when using this field.
    /// The 'InvocationLabel' is abstract and does not represent the actual
    /// value that seL4 system calls use as it is dependent on the kernel
    /// configuration. When we convert this invocation to a list of bytes, we
    /// need to use 'label_raw' instead.
    label: InvocationLabel,
    label_raw: u32,
    args: InvocationArgs,
    repeat: Option<(u32, InvocationArgs)>,
}

impl Invocation {
    pub fn new(config: &Config, args: InvocationArgs) -> Invocation {
        let label = args.to_label(config);
        Invocation {
            label,
            label_raw: label.to_value(),
            args,
            repeat: None,
        }
    }

    /// Convert our higher-level representation of a seL4 invocation
    /// into raw bytes that will be given to the monitor to interpret
    /// at runtime.
    /// Appends to the given data
    pub fn add_raw_invocation(&self, config: &Config, data: &mut Vec<u8>) {
        let (service, args, extra_caps): (u64, Vec<u64>, Vec<u64>) =
            self.args.clone().get_args(config);

        // To potentionally save some allocation, we reserve enough space for all the invocation args
        data.reserve(2 + args.len() * 8 + extra_caps.len() * 8);

        let mut tag = Invocation::message_info_new(
            self.label_raw as u64,
            0,
            extra_caps.len() as u64,
            args.len() as u64,
        );
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

        if let Some((_, repeat)) = self.repeat.clone() {
            // Assert that the variant of the invocation arguments is the
            // same as the repeat invocation argument variant.
            assert!(std::mem::discriminant(&self.args) == std::mem::discriminant(&repeat));

            let (repeat_service, repeat_args, repeat_extra_caps) = repeat.get_args(config);
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
        format!("         {:<20} {}", field_name, value)
    }

    fn fmt_field_cap(
        field_name: &'static str,
        cap: u64,
        cap_lookup: &HashMap<u64, String>,
    ) -> String {
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
    pub fn report_fmt<W: Write>(
        &self,
        f: &mut BufWriter<W>,
        config: &Config,
        cap_lookup: &HashMap<u64, String>,
    ) {
        let mut arg_strs = Vec::new();
        let (service, service_str) = match self.args {
            InvocationArgs::UntypedRetype {
                untyped,
                object_type,
                size_bits,
                root,
                node_index,
                node_depth,
                node_offset,
                num_objects,
            } => {
                arg_strs.push(object_type.format(config));
                let sz_fmt = if size_bits == 0 {
                    String::from("N/A")
                } else {
                    format!("0x{:x}", 1 << size_bits)
                };
                arg_strs.push(Invocation::fmt_field_str(
                    "size_bits",
                    format!("{} ({})", size_bits, sz_fmt),
                ));
                arg_strs.push(Invocation::fmt_field_cap("root", root, cap_lookup));
                arg_strs.push(Invocation::fmt_field("node_index", node_index));
                arg_strs.push(Invocation::fmt_field("node_depth", node_depth));
                arg_strs.push(Invocation::fmt_field("node_offset", node_offset));
                arg_strs.push(Invocation::fmt_field("num_objects", num_objects));
                (untyped, cap_lookup.get(&untyped).unwrap())
            }
            InvocationArgs::TcbSetSchedParams {
                tcb,
                authority,
                mcp,
                priority,
                sched_context,
                fault_ep,
            } => {
                arg_strs.push(Invocation::fmt_field_cap(
                    "authority",
                    authority,
                    cap_lookup,
                ));
                arg_strs.push(Invocation::fmt_field("mcp", mcp));
                arg_strs.push(Invocation::fmt_field("priority", priority));
                arg_strs.push(Invocation::fmt_field_cap(
                    "sched_context",
                    sched_context,
                    cap_lookup,
                ));
                arg_strs.push(Invocation::fmt_field_cap("fault_ep", fault_ep, cap_lookup));
                (tcb, cap_lookup.get(&tcb).unwrap())
            }
            InvocationArgs::TcbSetSpace {
                tcb,
                fault_ep,
                cspace_root,
                cspace_root_data,
                vspace_root,
                vspace_root_data,
            } => {
                arg_strs.push(Invocation::fmt_field_cap("fault_ep", fault_ep, cap_lookup));
                arg_strs.push(Invocation::fmt_field_cap(
                    "cspace_root",
                    cspace_root,
                    cap_lookup,
                ));
                arg_strs.push(Invocation::fmt_field("cspace_root_data", cspace_root_data));
                arg_strs.push(Invocation::fmt_field_cap(
                    "vspace_root",
                    vspace_root,
                    cap_lookup,
                ));
                arg_strs.push(Invocation::fmt_field("vspace_root_data", vspace_root_data));
                (tcb, cap_lookup.get(&tcb).unwrap())
            }
            InvocationArgs::TcbSetIpcBuffer {
                tcb,
                buffer,
                buffer_frame,
            } => {
                arg_strs.push(Invocation::fmt_field_hex("buffer", buffer));
                arg_strs.push(Invocation::fmt_field_cap(
                    "buffer_frame",
                    buffer_frame,
                    cap_lookup,
                ));
                (tcb, cap_lookup.get(&tcb).unwrap())
            }
            InvocationArgs::TcbResume { tcb } => (tcb, cap_lookup.get(&tcb).unwrap()),
            InvocationArgs::TcbWriteRegisters {
                tcb,
                resume,
                arch_flags,
                ref regs,
                ..
            } => {
                arg_strs.push(Invocation::fmt_field_bool("resume", resume));
                arg_strs.push(Invocation::fmt_field("arch_flags", arch_flags as u64));

                let reg_strs = regs
                    .iter()
                    .map(|(field, val)| Invocation::fmt_field_reg(field, *val))
                    .collect::<Vec<_>>();
                arg_strs.push(Invocation::fmt_field_str("regs", reg_strs[0].clone()));
                for s in &reg_strs[1..] {
                    arg_strs.push(format!("                              {}", s));
                }

                (tcb, cap_lookup.get(&tcb).unwrap())
            }
            InvocationArgs::TcbBindNotification { tcb, notification } => {
                arg_strs.push(Invocation::fmt_field_cap(
                    "notification",
                    notification,
                    cap_lookup,
                ));
                (tcb, cap_lookup.get(&tcb).unwrap())
            }
            InvocationArgs::AsidPoolAssign { asid_pool, vspace } => {
                arg_strs.push(Invocation::fmt_field_cap("vspace", vspace, cap_lookup));
                (asid_pool, cap_lookup.get(&asid_pool).unwrap())
            }
            InvocationArgs::IrqControlGetTrigger {
                irq_control,
                irq,
                trigger,
                dest_root,
                dest_index,
                dest_depth,
            } => {
                arg_strs.push(Invocation::fmt_field("irq", irq));
                arg_strs.push(Invocation::fmt_field("trigger", trigger as u64));
                arg_strs.push(Invocation::fmt_field_cap(
                    "dest_root",
                    dest_root,
                    cap_lookup,
                ));
                arg_strs.push(Invocation::fmt_field("dest_index", dest_index));
                arg_strs.push(Invocation::fmt_field("dest_depth", dest_depth));
                (irq_control, cap_lookup.get(&irq_control).unwrap())
            }
            InvocationArgs::IrqHandlerSetNotification {
                irq_handler,
                notification,
            } => {
                arg_strs.push(Invocation::fmt_field_cap(
                    "notification",
                    notification,
                    cap_lookup,
                ));
                (irq_handler, cap_lookup.get(&irq_handler).unwrap())
            }
            InvocationArgs::PageTableMap {
                page_table,
                vspace,
                vaddr,
                attr,
            } => {
                arg_strs.push(Invocation::fmt_field_cap("vspace", vspace, cap_lookup));
                arg_strs.push(Invocation::fmt_field_hex("vaddr", vaddr));
                arg_strs.push(Invocation::fmt_field("attr", attr));
                (page_table, cap_lookup.get(&page_table).unwrap())
            }
            InvocationArgs::PageMap {
                page,
                vspace,
                vaddr,
                rights,
                attr,
            } => {
                arg_strs.push(Invocation::fmt_field_cap("vspace", vspace, cap_lookup));
                arg_strs.push(Invocation::fmt_field_hex("vaddr", vaddr));
                arg_strs.push(Invocation::fmt_field("rights", rights));
                arg_strs.push(Invocation::fmt_field("attr", attr));
                (page, cap_lookup.get(&page).unwrap())
            }
            InvocationArgs::CnodeCopy {
                cnode,
                dest_index,
                dest_depth,
                src_root,
                src_obj,
                src_depth,
                rights,
            } => {
                arg_strs.push(Invocation::fmt_field("dest_index", dest_index));
                arg_strs.push(Invocation::fmt_field("dest_depth", dest_depth));
                arg_strs.push(Invocation::fmt_field_cap("src_root", src_root, cap_lookup));
                arg_strs.push(Invocation::fmt_field_cap("src_obj", src_obj, cap_lookup));
                arg_strs.push(Invocation::fmt_field("src_depth", src_depth));
                arg_strs.push(Invocation::fmt_field("rights", rights));
                (cnode, cap_lookup.get(&cnode).unwrap())
            }
            InvocationArgs::CnodeMint {
                cnode,
                dest_index,
                dest_depth,
                src_root,
                src_obj,
                src_depth,
                rights,
                badge,
            } => {
                arg_strs.push(Invocation::fmt_field("dest_index", dest_index));
                arg_strs.push(Invocation::fmt_field("dest_depth", dest_depth));
                arg_strs.push(Invocation::fmt_field_cap("src_root", src_root, cap_lookup));
                arg_strs.push(Invocation::fmt_field_cap("src_obj", src_obj, cap_lookup));
                arg_strs.push(Invocation::fmt_field("src_depth", src_depth));
                arg_strs.push(Invocation::fmt_field("rights", rights));
                arg_strs.push(Invocation::fmt_field("badge", badge));
                (cnode, cap_lookup.get(&cnode).unwrap())
            }
            InvocationArgs::SchedControlConfigureFlags {
                sched_control,
                sched_context,
                budget,
                period,
                extra_refills,
                badge,
                flags,
            } => {
                arg_strs.push(Invocation::fmt_field_cap(
                    "schedcontext",
                    sched_context,
                    cap_lookup,
                ));
                arg_strs.push(Invocation::fmt_field("budget", budget));
                arg_strs.push(Invocation::fmt_field("period", period));
                arg_strs.push(Invocation::fmt_field("extra_refills", extra_refills));
                arg_strs.push(Invocation::fmt_field("badge", badge));
                arg_strs.push(Invocation::fmt_field("flags", flags));
                (sched_control, &"None".to_string())
            }
            InvocationArgs::ArmVcpuSetTcb { vcpu, tcb } => {
                arg_strs.push(Invocation::fmt_field_cap("tcb", tcb, cap_lookup));
                (vcpu, cap_lookup.get(&vcpu).unwrap())
            }
        };
        _ = writeln!(
            f,
            "{:<20} - {:<17} - 0x{:016x} ({})\n{}",
            self.object_type(),
            self.method_name(),
            service,
            service_str,
            arg_strs.join("\n")
        );
        if let Some((count, _)) = self.repeat {
            _ = writeln!(f, "      REPEAT: count={}", count);
        }
    }

    fn object_type(&self) -> &'static str {
        match self.label {
            InvocationLabel::UntypedRetype => "Untyped",
            InvocationLabel::TcbSetSchedParams
            | InvocationLabel::TcbSetSpace
            | InvocationLabel::TcbSetIpcBuffer
            | InvocationLabel::TcbResume
            | InvocationLabel::TcbWriteRegisters
            | InvocationLabel::TcbBindNotification => "TCB",
            InvocationLabel::ArmAsidPoolAssign | InvocationLabel::RiscvAsidPoolAssign => {
                "ASID Pool"
            }
            InvocationLabel::ArmIrqIssueIrqHandlerTrigger
            | InvocationLabel::RiscvIrqIssueIrqHandlerTrigger => "IRQ Control",
            InvocationLabel::IrqSetIrqHandler => "IRQ Handler",
            InvocationLabel::ArmPageTableMap | InvocationLabel::RiscvPageTableMap => "Page Table",
            InvocationLabel::ArmPageMap | InvocationLabel::RiscvPageMap => "Page",
            InvocationLabel::CnodeCopy | InvocationLabel::CnodeMint => "CNode",
            InvocationLabel::SchedControlConfigureFlags => "SchedControl",
            InvocationLabel::ArmVcpuSetTcb => "VCPU",
            _ => panic!(
                "Internal error: unexpected label when getting object type '{:?}'",
                self.label
            ),
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
            InvocationLabel::ArmAsidPoolAssign | InvocationLabel::RiscvAsidPoolAssign => "Assign",
            InvocationLabel::ArmIrqIssueIrqHandlerTrigger
            | InvocationLabel::RiscvIrqIssueIrqHandlerTrigger => "Get",
            InvocationLabel::IrqSetIrqHandler => "SetNotification",
            InvocationLabel::ArmPageTableMap
            | InvocationLabel::ArmPageMap
            | InvocationLabel::RiscvPageTableMap
            | InvocationLabel::RiscvPageMap => "Map",
            InvocationLabel::CnodeCopy => "Copy",
            InvocationLabel::CnodeMint => "Mint",
            InvocationLabel::SchedControlConfigureFlags => "ConfigureFlags",
            InvocationLabel::ArmVcpuSetTcb => "VCPUSetTcb",
            _ => panic!(
                "Internal error: unexpected label when getting method name '{:?}'",
                self.label
            ),
        }
    }
}

impl InvocationArgs {
    fn to_label(&self, config: &Config) -> InvocationLabel {
        match self {
            InvocationArgs::UntypedRetype { .. } => InvocationLabel::UntypedRetype,
            InvocationArgs::TcbSetSchedParams { .. } => InvocationLabel::TcbSetSchedParams,
            InvocationArgs::TcbSetSpace { .. } => InvocationLabel::TcbSetSpace,
            InvocationArgs::TcbSetIpcBuffer { .. } => InvocationLabel::TcbSetIpcBuffer,
            InvocationArgs::TcbResume { .. } => InvocationLabel::TcbResume,
            InvocationArgs::TcbWriteRegisters { .. } => InvocationLabel::TcbWriteRegisters,
            InvocationArgs::TcbBindNotification { .. } => InvocationLabel::TcbBindNotification,
            InvocationArgs::AsidPoolAssign { .. } => match config.arch {
                Arch::Aarch64 => InvocationLabel::ArmAsidPoolAssign,
                Arch::Riscv64 => InvocationLabel::RiscvAsidPoolAssign,
            },
            InvocationArgs::IrqControlGetTrigger { .. } => match config.arch {
                Arch::Aarch64 => InvocationLabel::ArmIrqIssueIrqHandlerTrigger,
                Arch::Riscv64 => InvocationLabel::RiscvIrqIssueIrqHandlerTrigger,
            },
            InvocationArgs::IrqHandlerSetNotification { .. } => InvocationLabel::IrqSetIrqHandler,
            InvocationArgs::PageTableMap { .. } => match config.arch {
                Arch::Aarch64 => InvocationLabel::ArmPageTableMap,
                Arch::Riscv64 => InvocationLabel::RiscvPageTableMap,
            },
            InvocationArgs::PageMap { .. } => match config.arch {
                Arch::Aarch64 => InvocationLabel::ArmPageMap,
                Arch::Riscv64 => InvocationLabel::RiscvPageMap,
            },
            InvocationArgs::CnodeCopy { .. } => InvocationLabel::CnodeCopy,
            InvocationArgs::CnodeMint { .. } => InvocationLabel::CnodeMint,
            InvocationArgs::SchedControlConfigureFlags { .. } => {
                InvocationLabel::SchedControlConfigureFlags
            }
            InvocationArgs::ArmVcpuSetTcb { .. } => InvocationLabel::ArmVcpuSetTcb,
        }
    }

    fn get_args(self, config: &Config) -> (u64, Vec<u64>, Vec<u64>) {
        match self {
            InvocationArgs::UntypedRetype {
                untyped,
                object_type,
                size_bits,
                root,
                node_index,
                node_depth,
                node_offset,
                num_objects,
            } => (
                untyped,
                vec![
                    object_type.value(config),
                    size_bits,
                    node_index,
                    node_depth,
                    node_offset,
                    num_objects,
                ],
                vec![root],
            ),
            InvocationArgs::TcbSetSchedParams {
                tcb,
                authority,
                mcp,
                priority,
                sched_context,
                fault_ep,
            } => (
                tcb,
                vec![mcp, priority],
                vec![authority, sched_context, fault_ep],
            ),
            InvocationArgs::TcbSetSpace {
                tcb,
                fault_ep,
                cspace_root,
                cspace_root_data,
                vspace_root,
                vspace_root_data,
            } => (
                tcb,
                vec![cspace_root_data, vspace_root_data],
                vec![fault_ep, cspace_root, vspace_root],
            ),
            InvocationArgs::TcbSetIpcBuffer {
                tcb,
                buffer,
                buffer_frame,
            } => (tcb, vec![buffer], vec![buffer_frame]),
            InvocationArgs::TcbResume { tcb } => (tcb, vec![], vec![]),
            InvocationArgs::TcbWriteRegisters {
                tcb,
                resume,
                arch_flags,
                regs,
                count,
            } => {
                // Here there are a couple of things going on.
                // The invocation arguments to do not correspond one-to-one to word size,
                // so we have to do some packing first.
                // This means that the resume and arch_flags arguments need to be packed into
                // a single word. We then add all the registers which are each the size of a word.
                let resume_byte = if resume { 1 } else { 0 };
                let flags: u64 = ((arch_flags as u64) << 8) | resume_byte;
                let mut args = vec![flags, count];
                let regs_values = regs.into_iter().map(|(_, value)| value);
                args.extend(regs_values);
                (tcb, args, vec![])
            }
            InvocationArgs::TcbBindNotification { tcb, notification } => {
                (tcb, vec![], vec![notification])
            }
            InvocationArgs::AsidPoolAssign { asid_pool, vspace } => {
                (asid_pool, vec![], vec![vspace])
            }
            InvocationArgs::IrqControlGetTrigger {
                irq_control,
                irq,
                trigger,
                dest_root,
                dest_index,
                dest_depth,
            } => (
                irq_control,
                vec![irq, trigger as u64, dest_index, dest_depth],
                vec![dest_root],
            ),
            InvocationArgs::IrqHandlerSetNotification {
                irq_handler,
                notification,
            } => (irq_handler, vec![], vec![notification]),
            InvocationArgs::PageTableMap {
                page_table,
                vspace,
                vaddr,
                attr,
            } => (page_table, vec![vaddr, attr], vec![vspace]),
            InvocationArgs::PageMap {
                page,
                vspace,
                vaddr,
                rights,
                attr,
            } => (page, vec![vaddr, rights, attr], vec![vspace]),
            InvocationArgs::CnodeCopy {
                cnode,
                dest_index,
                dest_depth,
                src_root,
                src_obj,
                src_depth,
                rights,
            } => (
                cnode,
                vec![dest_index, dest_depth, src_obj, src_depth, rights],
                vec![src_root],
            ),
            InvocationArgs::CnodeMint {
                cnode,
                dest_index,
                dest_depth,
                src_root,
                src_obj,
                src_depth,
                rights,
                badge,
            } => (
                cnode,
                vec![dest_index, dest_depth, src_obj, src_depth, rights, badge],
                vec![src_root],
            ),
            InvocationArgs::SchedControlConfigureFlags {
                sched_control,
                sched_context,
                budget,
                period,
                extra_refills,
                badge,
                flags,
            } => (
                sched_control,
                vec![budget, period, extra_refills, badge, flags],
                vec![sched_context],
            ),
            InvocationArgs::ArmVcpuSetTcb { vcpu, tcb } => (vcpu, vec![], vec![tcb]),
        }
    }
}

#[derive(Clone)]
#[allow(dead_code, clippy::large_enum_variant)]
pub enum InvocationArgs {
    UntypedRetype {
        untyped: u64,
        object_type: ObjectType,
        size_bits: u64,
        root: u64,
        node_index: u64,
        node_depth: u64,
        node_offset: u64,
        num_objects: u64,
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
        regs: Vec<(&'static str, u64)>,
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
        trigger: IrqTrigger,
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
    CnodeCopy {
        cnode: u64,
        dest_index: u64,
        dest_depth: u64,
        src_root: u64,
        src_obj: u64,
        src_depth: u64,
        rights: u64,
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
    },
    ArmVcpuSetTcb {
        vcpu: u64,
        tcb: u64,
    },
}
