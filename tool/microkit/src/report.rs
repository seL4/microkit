//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::{fs::File, io::Write};

use sel4_capdl_initializer_types::Object;

use crate::{
    capdl::{
        spec::{
            capdl_cap_badge, capdl_cap_rights, capdl_obj_human_name, capdl_obj_physical_size_bits,
            capdl_rights_to_human_repr,
        },
        CapDLSpecContainer, TcbBoundSlot,
    },
    sel4::{Arch, ArmRiscvIrqTrigger, Config, X86IoapicIrqPolarity, X86IoapicIrqTrigger},
};

pub fn write_report(
    spec_container: &CapDLSpecContainer,
    kernel_config: &Config,
    output_path: &str,
) {
    let mut report_file = File::create(output_path).expect("Cannot create report file");

    report_file
        .write_all(b"# Initial Task (CapDL Initialiser) Details\n")
        .unwrap();

    report_file.write_all(b"\n# IRQ Details\n").unwrap();
    for irq in spec_container.spec.irqs.iter() {
        let irq_num = irq.irq;
        let handler = spec_container.get_root_object(irq.handler).unwrap();

        report_file
            .write_all(format!("\t- IRQ: '{}'\n", handler.name.as_ref().unwrap()).as_bytes())
            .unwrap();

        match &handler.object {
            Object::ArmIrq(arm_irq) => {
                report_file
                    .write_all(format!("\t\t* Number: {}\n", irq_num.0).as_bytes())
                    .unwrap();
                report_file
                    .write_all(
                        format!(
                            "\t\t* Trigger: {}\n",
                            ArmRiscvIrqTrigger::from(arm_irq.extra.trigger).human_name()
                        )
                        .as_bytes(),
                    )
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* CPU: {}\n", arm_irq.extra.target.0).as_bytes())
                    .unwrap();
            }
            Object::RiscvIrq(riscv_irq) => {
                report_file
                    .write_all(format!("\t\t* Number: {}\n", irq_num.0).as_bytes())
                    .unwrap();
                report_file
                    .write_all(
                        format!(
                            "\t\t* Trigger: {}\n",
                            ArmRiscvIrqTrigger::from(riscv_irq.extra.trigger).human_name()
                        )
                        .as_bytes(),
                    )
                    .unwrap();
            }
            Object::IrqMsi(irq_msi) => {
                report_file
                    .write_all(format!("\t\t* Vector: {}\n", irq_num.0).as_bytes())
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* PCI Bus: {}\n", irq_msi.extra.pci_bus.0).as_bytes())
                    .unwrap();
                report_file
                    .write_all(
                        format!("\t\t* PCI Device: {}\n", irq_msi.extra.pci_dev.0).as_bytes(),
                    )
                    .unwrap();
                report_file
                    .write_all(
                        format!("\t\t* PCI Function: {}\n", irq_msi.extra.pci_func.0).as_bytes(),
                    )
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* Handle: {}\n", irq_msi.extra.handle.0).as_bytes())
                    .unwrap();
            }
            Object::IrqIOApic(irq_ioapic) => {
                report_file
                    .write_all(format!("\t\t* Vector: {}\n", irq_num.0).as_bytes())
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* IOAPIC: {}\n", irq_ioapic.extra.ioapic.0).as_bytes())
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* Pin: {}\n", irq_ioapic.extra.pin.0).as_bytes())
                    .unwrap();
                report_file
                    .write_all(
                        format!(
                            "\t\t* Trigger: {}\n",
                            X86IoapicIrqTrigger::from(irq_ioapic.extra.level.0).human_name()
                        )
                        .as_bytes(),
                    )
                    .unwrap();
                report_file
                    .write_all(
                        format!(
                            "\t\t* Polarity: {}\n",
                            X86IoapicIrqPolarity::from(irq_ioapic.extra.polarity.0).human_name()
                        )
                        .as_bytes(),
                    )
                    .unwrap();
            }
            _ => unreachable!("internal bug: object is not IRQ!"),
        };
        report_file.write_all(b"\n").unwrap();
    }

    report_file.write_all(b"\n# TCB Details\n").unwrap();
    let tcb_objects = spec_container
        .spec
        .objects
        .iter()
        .filter(|named_object| matches!(named_object.object, Object::Tcb(_)));
    for named_tcb_objects in tcb_objects {
        report_file
            .write_all(
                format!("\t- TCB: '{}'\n", named_tcb_objects.name.as_ref().unwrap()).as_bytes(),
            )
            .unwrap();
        match &named_tcb_objects.object {
            Object::Tcb(tcb) => {
                report_file
                    .write_all(format!("\t\t* IP: 0x{:x}\n", tcb.extra.ip.0).as_bytes())
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* SP: 0x{:x}\n", tcb.extra.sp.0).as_bytes())
                    .unwrap();
                report_file
                    .write_all(
                        format!("\t\t* IPC Buffer: 0x{:x}\n", tcb.extra.ipc_buffer_addr.0)
                            .as_bytes(),
                    )
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* Priority: {}\n", tcb.extra.prio).as_bytes())
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* CPU Affinity: {}\n", tcb.extra.affinity.0).as_bytes())
                    .unwrap();

                report_file.write_all(b"\t\t* Bound Objects:\n").unwrap();
                for cte in tcb.slots.iter() {
                    let slot_enum = TcbBoundSlot::from(cte.slot.0);
                    let object_name = &spec_container.get_root_object(cte.cap.obj()).unwrap().name;
                    let prefix = match slot_enum {
                        TcbBoundSlot::CSpace => "CSpace",
                        TcbBoundSlot::VSpace => "VSpace",
                        TcbBoundSlot::IpcBuffer => "IPC Buffer",
                        TcbBoundSlot::FaultEp => "Fault Endpoint",
                        TcbBoundSlot::SchedContext => "Scheduling Context",
                        TcbBoundSlot::BoundNotification => "Notification",
                        TcbBoundSlot::VCpu => "VCpu",
                        TcbBoundSlot::X86Eptpml4 => "x86 EPT PML4",
                    };

                    report_file
                        .write_all(
                            format!("\t\t\t-> {}: '{}'\n", prefix, object_name.as_ref().unwrap())
                                .as_bytes(),
                        )
                        .unwrap();
                }
            }
            _ => unreachable!("internal bug: object is not TCB!"),
        }
        report_file.write_all(b"\n").unwrap();
    }

    report_file.write_all(b"\n# CNode Details\n").unwrap();
    let cnode_objects = spec_container
        .spec
        .objects
        .iter()
        .filter(|named_object| matches!(named_object.object, Object::CNode(_)));
    for named_cnode_object in cnode_objects {
        report_file
            .write_all(
                format!(
                    "\t- CNode: '{}'\n",
                    named_cnode_object.name.as_ref().unwrap()
                )
                .as_bytes(),
            )
            .unwrap();
        for cte in named_cnode_object.object.slots().unwrap() {
            let to_object = spec_container.get_root_object(cte.cap.obj()).unwrap();
            report_file
                .write_all(format!("\t\t* Slot: {}\n", cte.slot.0).as_bytes())
                .unwrap();

            report_file
                .write_all(
                    format!("\t\t\t-> Object: '{}'\n", to_object.name.as_ref().unwrap()).as_bytes(),
                )
                .unwrap();
            let rights_maybe = capdl_cap_rights(&cte.cap);
            if rights_maybe.is_some() {
                report_file
                    .write_all(
                        format!(
                            "\t\t\t-> Rights: {}\n",
                            capdl_rights_to_human_repr(&rights_maybe.unwrap())
                        )
                        .as_bytes(),
                    )
                    .unwrap();
            }
            let badge_maybe = capdl_cap_badge(&cte.cap);
            if badge_maybe.is_some() {
                report_file
                    .write_all(
                        format!("\t\t\t-> Badge: 0x{:x}\n", badge_maybe.unwrap().0).as_bytes(),
                    )
                    .unwrap();
            }
        }
        report_file.write_all(b"\n").unwrap();
    }

    report_file
        .write_all(b"\n# Architecture Specific Details\n")
        .unwrap();
    match kernel_config.arch {
        Arch::Aarch64 => {
            let is_smc = spec_container
                .spec
                .objects
                .iter()
                .filter(|named_object| matches!(named_object.object, Object::ArmSmc))
                .count()
                > 0;
            if is_smc {
                report_file
                    .write_all(b"ARM SMC access is granted to userspace.\n")
                    .unwrap();
            }
        }
        Arch::X86_64 => {
            let ioports = spec_container
                .spec
                .objects
                .iter()
                .filter(|named_object| matches!(named_object.object, Object::IOPorts(_)));

            for named_ioport_object in ioports {
                report_file
                    .write_all(
                        format!(
                            "\t- {}: '{}'\n",
                            capdl_obj_human_name(&named_ioport_object.object, kernel_config),
                            named_ioport_object.name.as_ref().unwrap()
                        )
                        .as_bytes(),
                    )
                    .unwrap();

                match &named_ioport_object.object {
                    Object::IOPorts(ioports) => {
                        report_file
                            .write_all(
                                format!("\t\t* Start Port: 0x{:x}\n", ioports.start_port.0)
                                    .as_bytes(),
                            )
                            .unwrap();
                        report_file
                            .write_all(
                                format!("\t\t* End Port: 0x{:x}\n", ioports.end_port.0).as_bytes(),
                            )
                            .unwrap();
                    }
                    _ => unreachable!("internal bug: object is not x86 I/O ports!"),
                }
            }
        }
        Arch::Riscv64 => {}
    }

    if kernel_config.arch != Arch::X86_64 {
        report_file
            .write_all(b"\n# Kernel Objects Details: ID, Type, Name, Physical Address\n")
            .unwrap();
    } else {
        report_file
            .write_all(b"\n# Kernel Objects Details: ID, Type, Name\n")
            .unwrap();
    }
    for (id, named_object) in spec_container.spec.objects.iter().enumerate() {
        if capdl_obj_physical_size_bits(&named_object.object, kernel_config) > 0 {
            if kernel_config.arch == Arch::X86_64 {
                report_file
                    .write_all(
                        format!(
                            "\t{} - {}: '{}'\n",
                            id,
                            capdl_obj_human_name(&named_object.object, kernel_config),
                            named_object.name.as_ref().unwrap(),
                        )
                        .as_bytes(),
                    )
                    .unwrap();
            } else {
                match spec_container.expected_allocations.get(&(id.into())) {
                    Some(allocation_details) => {
                        report_file
                            .write_all(
                                format!(
                                    "\t{} - {}: '{}' @ 0x{:0>12x}\n",
                                    id,
                                    capdl_obj_human_name(&named_object.object, kernel_config),
                                    named_object.name.as_ref().unwrap(),
                                    allocation_details.paddr
                                )
                                .as_bytes(),
                            )
                            .unwrap();
                    }
                    None => {
                        report_file
                            .write_all(
                                format!(
                                    "\t{} - {}: '{}' @ <Cannot be allocated/Previous fatal error>\n",
                                    id,
                                    capdl_obj_human_name(&named_object.object, kernel_config),
                                    named_object.name.as_ref().unwrap(),
                                )
                                .as_bytes(),
                            )
                            .unwrap();
                    }
                }
            }
        }
    }
}
