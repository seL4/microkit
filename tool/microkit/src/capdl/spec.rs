//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//
use core::ops::Range;
use sel4_capdl_initializer_types::{
    object::{AsidPool, IOPorts, SchedContext},
    CapTableEntry, Word,
};
use serde::{Deserialize, Serialize};

use crate::{
    capdl::SLOT_BITS,
    sel4::{Config, ObjectType, PageSize},
};

#[derive(Clone, Eq, PartialEq)]
pub struct ExpectedAllocation {
    pub ut_idx: usize,
    pub paddr: u64,
}

#[derive(Serialize, Clone, Eq, PartialEq)]
pub struct NamedObject {
    pub name: String,
    pub object: CapDLObject,

    // Internal Microkit tool use only, to keep tabs of
    // where objects will be allocated for the report.
    #[serde(skip_serializing)]
    pub expected_alloc: Option<ExpectedAllocation>,
}

#[derive(Serialize, Clone, Eq, PartialEq)]
pub enum FrameInit {
    Fill(Fill),
}

#[derive(Serialize, Clone, Eq, PartialEq)]
pub struct Fill {
    pub entries: Vec<FillEntry>,
}

#[derive(Serialize, Clone, Eq, PartialEq)]
pub struct FillEntry {
    pub range: Range<usize>,
    pub content: FillEntryContent,
}

#[derive(Serialize, Clone, Eq, PartialEq)]
pub enum FillEntryContent {
    Data(ElfContent),
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq)]
pub struct ElfContent {
    pub elf_id: usize,
    pub elf_seg_idx: usize,
    pub elf_seg_data_range: Range<usize>,
}

#[derive(Serialize, Clone, Eq, PartialEq)]
pub enum CapDLObject {
    Endpoint,
    Notification,
    CNode(capdl_object::CNode),
    Tcb(capdl_object::Tcb),
    VCpu,
    Frame(capdl_object::Frame),
    PageTable(capdl_object::PageTable),
    AsidPool(AsidPool),
    ArmIrq(capdl_object::ArmIrq),
    IrqMsi(capdl_object::IrqMsi),
    IrqIOApic(capdl_object::IrqIOApic),
    RiscvIrq(capdl_object::RiscvIrq),
    IOPorts(IOPorts),
    SchedContext(SchedContext),
    Reply,
    ArmSmc,
}

impl CapDLObject {
    pub fn paddr(&self) -> Option<usize> {
        match self {
            CapDLObject::Frame(obj) => obj.paddr,
            _ => None,
        }
    }

    /// CNode and SchedContext are quirky as they have variable size.
    pub fn physical_size_bits(&self, sel4_config: &Config) -> u64 {
        match self {
            CapDLObject::Endpoint => ObjectType::Endpoint.fixed_size_bits(sel4_config).unwrap(),
            CapDLObject::Notification => ObjectType::Notification
                .fixed_size_bits(sel4_config)
                .unwrap(),
            CapDLObject::CNode(cnode) => cnode.size_bits as u64 + SLOT_BITS,
            CapDLObject::Tcb(_) => ObjectType::Tcb.fixed_size_bits(sel4_config).unwrap(),
            CapDLObject::VCpu => ObjectType::Vcpu.fixed_size_bits(sel4_config).unwrap(),
            CapDLObject::Frame(frame) => frame.size_bits as u64,
            CapDLObject::PageTable(pt) => {
                if pt.is_root {
                    ObjectType::VSpace.fixed_size_bits(sel4_config).unwrap()
                } else {
                    ObjectType::PageTable.fixed_size_bits(sel4_config).unwrap()
                }
            }
            CapDLObject::AsidPool(_) => ObjectType::AsidPool.fixed_size_bits(sel4_config).unwrap(),
            CapDLObject::SchedContext(sched_context) => sched_context.size_bits as u64,
            CapDLObject::Reply => ObjectType::Reply.fixed_size_bits(sel4_config).unwrap(),
            _ => 0,
        }
    }

    pub fn get_cap_entries(&self) -> Option<&Vec<CapTableEntry>> {
        match self {
            CapDLObject::CNode(cnode) => Some(&cnode.slots),
            CapDLObject::Tcb(tcb) => Some(&tcb.slots),
            CapDLObject::PageTable(page_table) => Some(&page_table.slots),
            CapDLObject::ArmIrq(arm_irq) => Some(&arm_irq.slots),
            CapDLObject::IrqMsi(irq_msi) => Some(&irq_msi.slots),
            CapDLObject::IrqIOApic(irq_ioapic) => Some(&irq_ioapic.slots),
            CapDLObject::RiscvIrq(riscv_irq) => Some(&riscv_irq.slots),
            _ => None,
        }
    }

    pub fn get_cap_entries_mut(&mut self) -> Option<&mut Vec<CapTableEntry>> {
        match self {
            CapDLObject::CNode(cnode) => Some(&mut cnode.slots),
            CapDLObject::Tcb(tcb) => Some(&mut tcb.slots),
            CapDLObject::PageTable(page_table) => Some(&mut page_table.slots),
            CapDLObject::ArmIrq(arm_irq) => Some(&mut arm_irq.slots),
            CapDLObject::IrqMsi(irq_msi) => Some(&mut irq_msi.slots),
            CapDLObject::IrqIOApic(irq_ioapic) => Some(&mut irq_ioapic.slots),
            CapDLObject::RiscvIrq(riscv_irq) => Some(&mut riscv_irq.slots),
            _ => None,
        }
    }

    pub fn human_name(&self, sel4_config: &Config) -> &str {
        match self {
            CapDLObject::Endpoint => "Endpoint",
            CapDLObject::Notification => "Notification",
            CapDLObject::CNode(_) => "CNode",
            CapDLObject::Tcb(_) => "TCB",
            CapDLObject::VCpu => "VCPU",
            CapDLObject::Frame(frame) => {
                if frame.size_bits == PageSize::Small.fixed_size_bits(sel4_config) as usize {
                    "Page(4 KiB)"
                } else if frame.size_bits == PageSize::Large.fixed_size_bits(sel4_config) as usize {
                    "Page(2 MiB)"
                } else {
                    unreachable!("unknown frame size bits {}", frame.size_bits);
                }
            }
            CapDLObject::PageTable(_) => "PageTable",
            CapDLObject::AsidPool(_) => "AsidPool",
            CapDLObject::ArmIrq(_) => "ARM IRQ",
            CapDLObject::IrqMsi(_) => "x86 MSI IRQ",
            CapDLObject::IrqIOApic(_) => "x86 IOAPIC IRQ",
            CapDLObject::RiscvIrq(_) => "RISC-V IRQ",
            CapDLObject::IOPorts(_) => "x86 I/O Ports",
            CapDLObject::SchedContext(_) => "SchedContext",
            CapDLObject::Reply => "Reply",
            CapDLObject::ArmSmc => "ARM SMC",
        }
    }
}

pub mod capdl_object {
    use sel4_capdl_initializer_types::{
        object::{ArmIrqExtraInfo, IrqIOApicExtraInfo, IrqMsiExtraInfo, RiscvIrqExtraInfo},
        CPtr,
    };

    use super::*;
    /// Any object that takes a size bits is in addition to the base size

    #[derive(Serialize, Clone, Eq, PartialEq)]
    pub struct CNode {
        pub size_bits: usize,
        pub slots: Vec<CapTableEntry>,
    }

    #[derive(Serialize, Clone, Eq, PartialEq)]
    pub struct Tcb {
        pub slots: Vec<CapTableEntry>,
        pub extra: TcbExtraInfo,
    }

    #[derive(Serialize, Clone, Eq, PartialEq)]
    pub struct TcbExtraInfo {
        pub ipc_buffer_addr: Word,

        pub affinity: Word,
        pub prio: u8,
        pub max_prio: u8,
        pub resume: bool,

        pub ip: Word,
        pub sp: Word,
        pub gprs: Vec<Word>,

        pub master_fault_ep: Option<CPtr>,
    }

    #[derive(Serialize, Clone, Eq, PartialEq)]
    pub struct Frame {
        pub size_bits: usize,
        pub paddr: Option<usize>,
        pub init: FrameInit,
    }

    #[derive(Serialize, Clone, Eq, PartialEq)]
    pub struct PageTable {
        pub x86_ept: bool,
        pub is_root: bool,
        pub level: Option<u8>,
        pub slots: Vec<CapTableEntry>,
    }

    #[derive(Serialize, Clone, Eq, PartialEq)]
    pub struct ArmIrq {
        pub slots: Vec<CapTableEntry>,
        pub extra: ArmIrqExtraInfo,
    }

    #[derive(Serialize, Clone, Eq, PartialEq)]
    pub struct IrqMsi {
        pub slots: Vec<CapTableEntry>,
        pub extra: IrqMsiExtraInfo,
    }

    #[derive(Serialize, Clone, Eq, PartialEq)]
    pub struct IrqIOApic {
        pub slots: Vec<CapTableEntry>,
        pub extra: IrqIOApicExtraInfo,
    }

    #[derive(Serialize, Clone, Eq, PartialEq)]
    pub struct RiscvIrq {
        pub slots: Vec<CapTableEntry>,
        pub extra: RiscvIrqExtraInfo,
    }
}
