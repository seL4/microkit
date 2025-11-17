//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::ops::Range;

use sel4_capdl_initializer_types::{Cap, Object, Rights, Word};
use serde::Serialize;

use crate::{
    capdl::{FrameFill, SLOT_BITS},
    sel4::{Config, ObjectType, PageSize},
};

#[derive(Clone, Serialize)]
pub struct ElfContent {
    pub elf_id: usize,
    pub elf_seg_idx: usize,
    pub elf_seg_data_range: Range<usize>,
}

/// CNode and SchedContext are quirky as they have variable size.
pub fn capdl_obj_physical_size_bits(obj: &Object<FrameFill>, sel4_config: &Config) -> u64 {
    match obj {
        Object::Endpoint => ObjectType::Endpoint.fixed_size_bits(sel4_config).unwrap(),
        Object::Notification => ObjectType::Notification
            .fixed_size_bits(sel4_config)
            .unwrap(),
        Object::CNode(cnode) => cnode.size_bits as u64 + SLOT_BITS,
        Object::Tcb(_) => ObjectType::Tcb.fixed_size_bits(sel4_config).unwrap(),
        Object::VCpu => ObjectType::Vcpu.fixed_size_bits(sel4_config).unwrap(),
        Object::Frame(frame) => frame.size_bits as u64,
        Object::PageTable(pt) => {
            if pt.is_root {
                ObjectType::VSpace.fixed_size_bits(sel4_config).unwrap()
            } else {
                ObjectType::PageTable.fixed_size_bits(sel4_config).unwrap()
            }
        }
        Object::AsidPool(_) => ObjectType::AsidPool.fixed_size_bits(sel4_config).unwrap(),
        Object::SchedContext(sched_context) => sched_context.size_bits as u64,
        Object::Reply => ObjectType::Reply.fixed_size_bits(sel4_config).unwrap(),
        _ => 0,
    }
}

pub fn capdl_obj_human_name(obj: &Object<FrameFill>, sel4_config: &Config) -> &'static str {
    match obj {
        Object::Endpoint => "Endpoint",
        Object::Notification => "Notification",
        Object::CNode(_) => "CNode",
        Object::Tcb(_) => "TCB",
        Object::VCpu => "VCPU",
        Object::Frame(frame) => {
            if frame.size_bits == PageSize::Small.fixed_size_bits(sel4_config) as u8 {
                "Page(4 KiB)"
            } else if frame.size_bits == PageSize::Large.fixed_size_bits(sel4_config) as u8 {
                "Page(2 MiB)"
            } else {
                unreachable!("unknown frame size bits {}", frame.size_bits);
            }
        }
        Object::PageTable(_) => "PageTable",
        Object::AsidPool(_) => "AsidPool",
        Object::ArmIrq(_) => "ARM IRQ",
        Object::IrqMsi(_) => "x86 MSI IRQ",
        Object::IrqIOApic(_) => "x86 IOAPIC IRQ",
        Object::RiscvIrq(_) => "RISC-V IRQ",
        Object::IOPorts(_) => "x86 I/O Ports",
        Object::SchedContext(_) => "SchedContext",
        Object::Reply => "Reply",
        Object::ArmSmc => "ARM SMC",
        Object::Untyped(_) => "Untyped",
        Object::Irq(_) => "IRQ",
    }
}

pub fn capdl_cap_badge(cap: &Cap) -> Option<Word> {
    match cap {
        Cap::Endpoint(endpoint) => Some(endpoint.badge),
        Cap::Notification(notification) => Some(notification.badge),
        _ => None,
    }
}

pub fn capdl_cap_rights(cap: &Cap) -> Option<Rights> {
    match cap {
        Cap::Endpoint(endpoint) => Some(endpoint.rights),
        Cap::Notification(notification) => Some(notification.rights),
        Cap::Frame(frame) => Some(frame.rights),
        _ => None,
    }
}

pub fn capdl_rights_to_human_repr(rights: &Rights) -> String {
    let mut repr: String = "".into();

    if rights.read {
        repr += "Read, ";
    }
    if rights.write {
        repr += "Write, ";
    }
    if rights.grant {
        repr += "Grant, ";
    }
    if rights.grant_reply {
        repr += "Grant Reply, ";
    }

    repr.pop();
    repr.pop();

    repr
}
