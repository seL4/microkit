//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use sel4_capdl_initializer_types::{
    cap,
    object::{ArmIrqExtraInfo, IrqIOApicExtraInfo, IrqMsiExtraInfo, RiscvIrqExtraInfo},
    Cap, IrqEntry, ObjectId,
};

use crate::{
    capdl::{spec::*, util::capdl_util_make_ntfn_cap, CapDLSpec},
    sdf::{SysIrq, SysIrqKind},
    sel4::{Arch, Config},
};

/// Create all the objects needed in the spec for the requested IRQ.
/// Returns an IRQ handler Cap for insertion into the PD's CSpace.
pub fn create_irq_handler_cap(
    spec: &mut CapDLSpec,
    sel4_config: &Config,
    pd_name: &str,
    pd_ntfn_obj_id: ObjectId,
    irq_desc: &SysIrq,
) -> Cap {
    // Create the IRQ object and add it to the special `irqs` vec in the spec.
    // This is a pseudo object so we can bind a cap to the IRQ
    let irq_obj_id = create_irq_obj(spec, sel4_config, pd_name, irq_desc);

    // Create the real IRQ in the separate IRQ vector.
    spec.irqs.push(IrqEntry {
        irq: irq_desc.irq_num(),
        handler: irq_obj_id,
    });

    // Bind IRQ into the PD's notification with the correct badge
    let pd_irq_ntfn_cap = capdl_util_make_ntfn_cap(pd_ntfn_obj_id, true, true, 1 << irq_desc.id);
    bind_irq_to_ntfn(spec, irq_obj_id, pd_irq_ntfn_cap);

    // Create a IRQ handler cap
    make_irq_handler_cap(sel4_config, irq_obj_id, &irq_desc.kind)
}

fn create_irq_obj(
    spec: &mut CapDLSpec,
    sel4_config: &Config,
    pd_name: &str,
    irq_desc: &SysIrq,
) -> ObjectId {
    let irq_inner_obj = match irq_desc.kind {
        SysIrqKind::Conventional { trigger, .. } => match sel4_config.arch {
            Arch::Aarch64 => CapDLObject::ArmIrq(capdl_object::ArmIrq {
                slots: [].to_vec(),
                extra: ArmIrqExtraInfo {
                    trigger: trigger as u64,
                    target: 0, // @billn revisit for SMP
                },
            }),
            Arch::Riscv64 => CapDLObject::RiscvIrq(capdl_object::RiscvIrq {
                slots: [].to_vec(),
                extra: RiscvIrqExtraInfo {
                    trigger: trigger as u64,
                },
            }),
            Arch::X86_64 => unreachable!(
                "create_irq_obj(): internal bug: ARM and RISC-V IRQs not supported on x86."
            ),
        },
        SysIrqKind::IOAPIC {
            ioapic,
            pin,
            trigger,
            polarity,
            ..
        } => CapDLObject::IrqIOApic(capdl_object::IrqIOApic {
            slots: [].to_vec(),
            extra: IrqIOApicExtraInfo {
                ioapic,
                pin,
                level: trigger as u64,
                polarity: polarity as u64,
            },
        }),
        SysIrqKind::MSI {
            pci_bus,
            pci_dev,
            pci_func,
            handle,
            ..
        } => CapDLObject::IrqMsi(capdl_object::IrqMsi {
            slots: [].to_vec(),
            extra: IrqMsiExtraInfo {
                handle,
                pci_bus,
                pci_dev,
                pci_func,
            },
        }),
    };
    let irq_obj = NamedObject {
        name: format!("irq_{}_{}", irq_desc.irq_num(), pd_name),
        object: irq_inner_obj,
        expected_alloc: None,
    };
    spec.add_root_object(irq_obj)
}

fn bind_irq_to_ntfn(spec: &mut CapDLSpec, irq_obj_id: ObjectId, ntfn_cap: Cap) {
    match &mut spec.get_root_object_mut(irq_obj_id).unwrap().object {
        CapDLObject::ArmIrq(arm_irq) => {
            arm_irq.slots.push((0, ntfn_cap));
        }
        CapDLObject::IrqMsi(irq_msi) => {
            irq_msi.slots.push((0, ntfn_cap));
        }
        CapDLObject::IrqIOApic(irq_ioapic) => {
            irq_ioapic.slots.push((0, ntfn_cap));
        }
        CapDLObject::RiscvIrq(riscv_irq) => {
            riscv_irq.slots.push((0, ntfn_cap));
        }
        _ => unreachable!(
            "bind_irq_to_ntfn(): internal bug: got non irq object id {} with name '{}'",
            irq_obj_id,
            spec.get_root_object(irq_obj_id).unwrap().name
        ),
    }
}

fn make_irq_handler_cap(sel4_config: &Config, irq_obj_id: ObjectId, irq_kind: &SysIrqKind) -> Cap {
    match irq_kind {
        SysIrqKind::Conventional { .. } => match sel4_config.arch {
            Arch::Aarch64 => Cap::ArmIrqHandler(cap::ArmIrqHandler { object: irq_obj_id }),
            Arch::Riscv64 => Cap::RiscvIrqHandler(cap::RiscvIrqHandler { object: irq_obj_id }),
            Arch::X86_64 => unreachable!(
                "make_irq_handler_cap(): internal bug: ARM and RISC-V IRQs not supported on x86."
            ),
        },
        SysIrqKind::IOAPIC { .. } => {
            Cap::IrqIOApicHandler(cap::IrqIOApicHandler { object: irq_obj_id })
        }
        SysIrqKind::MSI { .. } => Cap::IrqMsiHandler(cap::IrqMsiHandler { object: irq_obj_id }),
    }
}
