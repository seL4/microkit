//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use crate::capdl::{
    builder::PD_CAP_SIZE,
    spec::{
        capdl_object::{CNode, Frame},
        CapDLObject, FrameInit, NamedObject,
    },
    CapDLSpec,
};
use sel4_capdl_initializer_types::{
    cap,
    object::{IOPorts, SchedContext, SchedContextExtraInfo},
    Cap, CapTableEntry, ObjectId, Rights,
};

// This module contains utility functions used by higher-level
// CapDL spec generation code. For simplicity, this code will trust
// all arguments given to it as it is only meant to be used internally
// in the CapDL implementation.

/// Create a frame object and add it to the spec, returns the
/// object number.
pub fn capdl_util_make_frame_obj(
    spec: &mut CapDLSpec,
    frame_init: FrameInit,
    name: &str,
    paddr: Option<usize>,
    size_bits: usize,
) -> ObjectId {
    let frame_inner_obj = CapDLObject::Frame(Frame {
        size_bits,
        paddr,
        init: frame_init,
    });
    let frame_obj = NamedObject {
        name: format!("frame_{name}"),
        object: frame_inner_obj,
        expected_alloc: None,
    };
    spec.add_root_object(frame_obj)
}

/// Create a frame capability from a frame object for mapping the frame in a VSpace
pub fn capdl_util_make_frame_cap(
    frame_obj_id: ObjectId,
    read: bool,
    write: bool,
    executable: bool,
    cached: bool,
) -> Cap {
    Cap::Frame(cap::Frame {
        object: frame_obj_id,
        rights: Rights {
            read,
            write,
            grant: false,
            grant_reply: false,
        },
        // This is not used on RISC-V, PTEs have no cached bit, see seL4_RISCV_VMAttributes.
        cached,
        // This is ignored on x86 by seL4. As the NX/XD bit that marks page as non-executable
        // is unsupported on old hardware.
        executable,
    })
}

pub fn capdl_util_make_tcb_cap(tcb_obj_id: ObjectId) -> Cap {
    Cap::Tcb(cap::Tcb { object: tcb_obj_id })
}

pub fn capdl_util_make_page_table_cap(pt_obj_id: ObjectId) -> Cap {
    Cap::PageTable(cap::PageTable { object: pt_obj_id })
}

// Given a TCB object ID, return that TCB's VSpace object ID.
pub fn capdl_util_get_vspace_id_from_tcb_id(spec: &CapDLSpec, tcb_obj_id: ObjectId) -> ObjectId {
    let tcb = match spec.get_root_object(tcb_obj_id) {
        Some(named_object) => {
            if let CapDLObject::Tcb(tcb) = &named_object.object {
                Some(tcb)
            } else {
                unreachable!("get_vspace_id_from_tcb_id(): internal bug: got a non TCB object id {} with name '{}'", tcb_obj_id, named_object.name);
            }
        }
        None => {
            unreachable!(
                "get_vspace_id_from_tcb_id(): internal bug: couldn't find tcb with given obj id."
            );
        }
    };
    let vspace_cap = tcb
        .unwrap()
        .slots
        .iter()
        .find(|&cte| matches!(&cte.1, Cap::PageTable(_)));
    vspace_cap.unwrap().1.obj()
}

pub fn capdl_util_get_frame_size_bits(spec: &CapDLSpec, frame_obj_id: ObjectId) -> usize {
    if let CapDLObject::Frame(frame) = &spec.get_root_object(frame_obj_id).unwrap().object {
        frame.size_bits
    } else {
        unreachable!(
            "internal bug: capdl_util_get_frame_size_bits() received a non Frame object ID"
        );
    }
}

pub fn capdl_util_make_endpoint_obj(
    spec: &mut CapDLSpec,
    pd_name: &str,
    is_fault: bool,
) -> ObjectId {
    let fault_ep_obj = NamedObject {
        name: format!("ep_{}{}", if is_fault { "fault_" } else { "" }, pd_name),
        object: CapDLObject::Endpoint,
        expected_alloc: None,
    };
    spec.add_root_object(fault_ep_obj)
}

pub fn capdl_util_make_endpoint_cap(
    ep_obj_id: ObjectId,
    read: bool,
    write: bool,
    grant: bool,
    badge: u64,
) -> Cap {
    Cap::Endpoint(cap::Endpoint {
        object: ep_obj_id,
        badge,
        rights: Rights {
            read,
            write,
            grant,
            grant_reply: false,
        },
    })
}

pub fn capdl_util_make_ntfn_obj(spec: &mut CapDLSpec, pd_name: &str) -> ObjectId {
    let ntfn_obj = NamedObject {
        name: format!("ntfn_{pd_name}"),
        object: CapDLObject::Notification,
        expected_alloc: None,
    };
    spec.add_root_object(ntfn_obj)
}

pub fn capdl_util_make_ntfn_cap(ntfn_obj_id: ObjectId, read: bool, write: bool, badge: u64) -> Cap {
    Cap::Notification(cap::Notification {
        object: ntfn_obj_id,
        badge,
        rights: Rights {
            read,
            write,
            // Irrelevant for notifications, seL4 manual v13.0.0 pg11
            grant: false,
            grant_reply: false,
        },
    })
}

pub fn capdl_util_make_reply_obj(spec: &mut CapDLSpec, pd_name: &str) -> ObjectId {
    let reply_obj = NamedObject {
        name: format!("reply_{pd_name}"),
        object: CapDLObject::Reply,
        expected_alloc: None,
    };
    spec.add_root_object(reply_obj)
}

pub fn capdl_util_make_reply_cap(reply_obj_id: ObjectId) -> Cap {
    Cap::Reply(cap::Reply {
        object: reply_obj_id,
    })
}

pub fn capdl_util_make_sc_obj(
    spec: &mut CapDLSpec,
    pd_name: &str,
    size_bits: usize,
    period: u64,
    budget: u64,
    badge: u64,
) -> ObjectId {
    let sc_inner_obj = CapDLObject::SchedContext(SchedContext {
        size_bits,
        extra: SchedContextExtraInfo {
            period,
            budget,
            badge,
        },
    });
    let sc_obj = NamedObject {
        name: format!("sched_context_{pd_name}"),
        object: sc_inner_obj,
        expected_alloc: None,
    };
    spec.add_root_object(sc_obj)
}

pub fn capdl_util_make_sc_cap(sc_obj_id: ObjectId) -> Cap {
    Cap::SchedContext(cap::SchedContext { object: sc_obj_id })
}

pub fn capdl_util_make_cnode_obj(
    spec: &mut CapDLSpec,
    pd_name: &str,
    size_bits: usize,
    slots: Vec<CapTableEntry>,
) -> ObjectId {
    let cnode_inner_obj = CapDLObject::CNode(CNode { size_bits, slots });
    let cnode_obj = NamedObject {
        name: format!("cnode_{pd_name}"),
        object: cnode_inner_obj,
        expected_alloc: None,
    };
    // Move monitor CSpace into spec and make a cap for it to insert into TCB later.
    spec.add_root_object(cnode_obj)
}

pub fn capdl_util_make_cnode_cap(cnode_obj_id: ObjectId, guard: u64, guard_size: u64) -> Cap {
    Cap::CNode(cap::CNode {
        object: cnode_obj_id,
        guard,

        guard_size,
    })
}

pub fn capdl_util_make_ioport_obj(
    spec: &mut CapDLSpec,
    pd_name: &str,
    start_addr: u64,
    size: u64,
) -> ObjectId {
    let ioport_inner_obj = CapDLObject::IOPorts(IOPorts {
        start_port: start_addr,
        end_port: start_addr + size - 1,
    });
    let ioport_obj = NamedObject {
        name: format!("ioports_0x{start_addr:x}_{pd_name}"),
        object: ioport_inner_obj,
        expected_alloc: None,
    };
    spec.add_root_object(ioport_obj)
}

pub fn capdl_util_make_ioport_cap(ioport_obj_id: ObjectId) -> Cap {
    Cap::IOPorts(cap::IOPorts {
        object: ioport_obj_id,
    })
}

pub fn capdl_util_insert_cap_into_cspace(
    spec: &mut CapDLSpec,
    cspace_obj_id: ObjectId,
    idx: usize,
    cap: Cap,
) {
    assert!(idx < PD_CAP_SIZE as usize);
    let cspace_obj = spec.get_root_object_mut(cspace_obj_id).unwrap();
    if let CapDLObject::CNode(cspace_inner_obj) = &mut cspace_obj.object {
        cspace_inner_obj.slots.push((idx, cap));
    } else {
        unreachable!("capdl_util_insert_cap_into_cspace(): internal bug: got a non CNode object id {} with name '{}'", cspace_obj_id, cspace_obj.name);
    }
}

pub fn capdl_util_make_vcpu_obj(spec: &mut CapDLSpec, name: &String) -> ObjectId {
    let vcpu_inner_obj = CapDLObject::VCpu;
    let vcpu_obj = NamedObject {
        name: format!("vcpu_{name}"),
        object: vcpu_inner_obj,
        expected_alloc: None,
    };
    spec.add_root_object(vcpu_obj)
}

pub fn capdl_util_make_vcpu_cap(vcpu_obj_id: ObjectId) -> Cap {
    Cap::VCpu(cap::VCpu {
        object: vcpu_obj_id,
    })
}

pub fn capdl_util_make_arm_smc_cap(arm_smc_obj_id: ObjectId) -> Cap {
    Cap::ArmSmc(cap::ArmSmc {
        object: arm_smc_obj_id,
    })
}
