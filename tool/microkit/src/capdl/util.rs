//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use crate::capdl::{builder::PD_CAP_SIZE, CapDLNamedObject, CapDLSpecContainer, FrameFill};
use sel4_capdl_initializer_types::{
    cap, object, Cap, CapSlot, CapTableEntry, Object, ObjectId, Rights, Word,
};

// This module contains utility functions used by higher-level
// CapDL spec generation code. For simplicity, this code will trust
// all arguments given to it as it is only meant to be used internally
// in the CapDL implementation.

pub fn capdl_util_make_cte(slot: u32, cap: Cap) -> CapTableEntry {
    CapTableEntry {
        slot: CapSlot(slot),
        cap,
    }
}

/// Create a frame object and add it to the spec, returns the
/// object number.
pub fn capdl_util_make_frame_obj(
    spec_container: &mut CapDLSpecContainer,
    frame_init: FrameFill,
    name: &str,
    paddr: Option<Word>,
    size_bits: u8,
) -> ObjectId {
    let frame_inner_obj = Object::Frame(object::Frame {
        size_bits,
        paddr,
        init: frame_init,
    });
    let frame_obj = CapDLNamedObject {
        name: format!("frame_{name}").into(),
        object: frame_inner_obj,
    };
    spec_container.add_root_object(frame_obj)
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
pub fn capdl_util_get_vspace_id_from_tcb_id(
    spec_container: &CapDLSpecContainer,
    tcb_obj_id: ObjectId,
) -> ObjectId {
    let tcb = match spec_container.get_root_object(tcb_obj_id) {
        Some(named_object) => {
            if let Object::Tcb(tcb) = &named_object.object {
                Some(tcb)
            } else {
                unreachable!("get_vspace_id_from_tcb_id(): internal bug: got a non TCB object id {} with name '{}'", usize::from(tcb_obj_id), named_object.name.as_ref().unwrap());
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
        .find(|&cte| matches!(&cte.cap, Cap::PageTable(_)));
    vspace_cap.unwrap().cap.obj()
}

pub fn capdl_util_get_frame_size_bits(
    spec_container: &CapDLSpecContainer,
    frame_obj_id: ObjectId,
) -> u8 {
    if let Object::Frame(frame) = &spec_container.get_root_object(frame_obj_id).unwrap().object {
        frame.size_bits
    } else {
        unreachable!(
            "internal bug: capdl_util_get_frame_size_bits() received a non Frame object ID"
        );
    }
}

pub fn capdl_util_make_endpoint_obj(
    spec_container: &mut CapDLSpecContainer,
    pd_name: &str,
    is_fault: bool,
) -> ObjectId {
    let fault_ep_obj = CapDLNamedObject {
        name: format!("ep_{}{}", if is_fault { "fault_" } else { "" }, pd_name).into(),
        object: Object::Endpoint,
    };
    spec_container.add_root_object(fault_ep_obj)
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
        badge: Word(badge),
        rights: Rights {
            read,
            write,
            grant,
            grant_reply: false,
        },
    })
}

pub fn capdl_util_make_ntfn_obj(
    spec_container: &mut CapDLSpecContainer,
    pd_name: &str,
) -> ObjectId {
    let ntfn_obj = CapDLNamedObject {
        name: format!("ntfn_{pd_name}").into(),
        object: Object::Notification,
    };
    spec_container.add_root_object(ntfn_obj)
}

pub fn capdl_util_make_ntfn_cap(ntfn_obj_id: ObjectId, read: bool, write: bool, badge: u64) -> Cap {
    Cap::Notification(cap::Notification {
        object: ntfn_obj_id,
        badge: Word(badge),
        rights: Rights {
            read,
            write,
            // Irrelevant for notifications, seL4 manual v13.0.0 pg11
            grant: false,
            grant_reply: false,
        },
    })
}

pub fn capdl_util_make_reply_obj(
    spec_container: &mut CapDLSpecContainer,
    pd_name: &str,
) -> ObjectId {
    let reply_obj = CapDLNamedObject {
        name: format!("reply_{pd_name}").into(),
        object: Object::Reply,
    };
    spec_container.add_root_object(reply_obj)
}

pub fn capdl_util_make_reply_cap(reply_obj_id: ObjectId) -> Cap {
    Cap::Reply(cap::Reply {
        object: reply_obj_id,
    })
}

pub fn capdl_util_make_sc_obj(
    spec_container: &mut CapDLSpecContainer,
    pd_name: &str,
    size_bits: u8,
    period: u64,
    budget: u64,
    badge: u64,
) -> ObjectId {
    let sc_inner_obj = Object::SchedContext(object::SchedContext {
        size_bits,
        extra: object::SchedContextExtraInfo {
            period,
            budget,
            badge: Word(badge),
        },
    });
    let sc_obj = CapDLNamedObject {
        name: format!("sched_context_{pd_name}").into(),
        object: sc_inner_obj,
    };
    spec_container.add_root_object(sc_obj)
}

pub fn capdl_util_make_sc_cap(sc_obj_id: ObjectId) -> Cap {
    Cap::SchedContext(cap::SchedContext { object: sc_obj_id })
}

pub fn capdl_util_make_cnode_obj(
    spec_container: &mut CapDLSpecContainer,
    pd_name: &str,
    size_bits: u8,
    slots: Vec<CapTableEntry>,
) -> ObjectId {
    let cnode_inner_obj = Object::CNode(object::CNode { size_bits, slots });
    let cnode_obj = CapDLNamedObject {
        name: format!("cnode_{pd_name}").into(),
        object: cnode_inner_obj,
    };
    // Move monitor CSpace into spec and make a cap for it to insert into TCB later.
    spec_container.add_root_object(cnode_obj)
}

pub fn capdl_util_make_cnode_cap(cnode_obj_id: ObjectId, guard: u64, guard_size: u8) -> Cap {
    Cap::CNode(cap::CNode {
        object: cnode_obj_id,
        guard: Word(guard),
        guard_size,
    })
}

pub fn capdl_util_make_ioport_obj(
    spec_container: &mut CapDLSpecContainer,
    pd_name: &str,
    start_addr: u64,
    size: u64,
) -> ObjectId {
    let ioport_inner_obj = Object::IOPorts(object::IOPorts {
        start_port: Word(start_addr),
        end_port: Word(start_addr + size - 1),
    });
    let ioport_obj = CapDLNamedObject {
        name: format!("ioports_0x{start_addr:x}_{pd_name}").into(),
        object: ioport_inner_obj,
    };
    spec_container.add_root_object(ioport_obj)
}

pub fn capdl_util_make_ioport_cap(ioport_obj_id: ObjectId) -> Cap {
    Cap::IOPorts(cap::IOPorts {
        object: ioport_obj_id,
    })
}

pub fn capdl_util_insert_cap_into_cspace(
    spec_container: &mut CapDLSpecContainer,
    cspace_obj_id: ObjectId,
    idx: u32,
    cap: Cap,
) {
    assert!(idx < PD_CAP_SIZE);
    let cspace_obj = spec_container.get_root_object_mut(cspace_obj_id).unwrap();
    if let Object::CNode(cspace_inner_obj) = &mut cspace_obj.object {
        cspace_inner_obj.slots.push(capdl_util_make_cte(idx, cap));
    } else {
        unreachable!("capdl_util_insert_cap_into_cspace(): internal bug: got a non CNode object id {} with name '{}'", usize::from(cspace_obj_id), cspace_obj.name.as_ref().unwrap());
    }
}

pub fn capdl_util_make_vcpu_obj(
    spec_container: &mut CapDLSpecContainer,
    name: &String,
) -> ObjectId {
    let vcpu_inner_obj = Object::VCpu;
    let vcpu_obj = CapDLNamedObject {
        name: format!("vcpu_{name}").into(),
        object: vcpu_inner_obj,
    };
    spec_container.add_root_object(vcpu_obj)
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
