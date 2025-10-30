//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::{fs::File, io::Write};

use crate::{
    capdl::{spec::CapDLObject, CapDLSpec, TcbBoundSlot},
    sel4::{Arch, ArmRiscvIrqTrigger, Config, X86IoapicIrqPolarity, X86IoapicIrqTrigger},
};

pub fn write_report(spec: &CapDLSpec, kernel_config: &Config, output_path: &str) {
    let mut report_file = File::create(output_path).expect("Cannot create report file");

    report_file
        .write_all(b"# Initial Task (CapDL Initialiser) Details\n")
        .unwrap();

    report_file.write_all(b"\n# IRQ Details\n").unwrap();
    for irq in spec.irqs.iter() {
        let irq_num = irq.irq;
        let handler = spec.get_root_object(irq.handler).unwrap();

        report_file
            .write_all(format!("\t- IRQ: '{}'\n", handler.name).as_bytes())
            .unwrap();

        match &handler.object {
            CapDLObject::ArmIrq(arm_irq) => {
                report_file
                    .write_all(format!("\t\t* Number: {irq_num}\n").as_bytes())
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
                    .write_all(format!("\t\t* CPU: {}\n", arm_irq.extra.target).as_bytes())
                    .unwrap();
            }
            CapDLObject::RiscvIrq(riscv_irq) => {
                report_file
                    .write_all(format!("\t\t* Number: {irq_num}\n").as_bytes())
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
            CapDLObject::IrqMsi(irq_msi) => {
                report_file
                    .write_all(format!("\t\t* Vector: {irq_num}\n").as_bytes())
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* PCI Bus: {}\n", irq_msi.extra.pci_bus).as_bytes())
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* PCI Device: {}\n", irq_msi.extra.pci_dev).as_bytes())
                    .unwrap();
                report_file
                    .write_all(
                        format!("\t\t* PCI Function: {}\n", irq_msi.extra.pci_func).as_bytes(),
                    )
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* Handle: {}\n", irq_msi.extra.handle).as_bytes())
                    .unwrap();
            }
            CapDLObject::IrqIOApic(irq_ioapic) => {
                report_file
                    .write_all(format!("\t\t* Vector: {irq_num}\n").as_bytes())
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* IOAPIC: {}\n", irq_ioapic.extra.ioapic).as_bytes())
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* Pin: {}\n", irq_ioapic.extra.pin).as_bytes())
                    .unwrap();
                report_file
                    .write_all(
                        format!(
                            "\t\t* Trigger: {}\n",
                            X86IoapicIrqTrigger::from(irq_ioapic.extra.level).human_name()
                        )
                        .as_bytes(),
                    )
                    .unwrap();
                report_file
                    .write_all(
                        format!(
                            "\t\t* Polarity: {}\n",
                            X86IoapicIrqPolarity::from(irq_ioapic.extra.polarity).human_name()
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
    let tcb_objects = spec
        .objects
        .iter()
        .filter(|named_object| matches!(named_object.object, CapDLObject::Tcb(_)));
    for named_tcb_objects in tcb_objects {
        report_file
            .write_all(format!("\t- TCB: '{}'\n", named_tcb_objects.name).as_bytes())
            .unwrap();
        match &named_tcb_objects.object {
            CapDLObject::Tcb(tcb) => {
                report_file
                    .write_all(format!("\t\t* IP: 0x{:x}\n", tcb.extra.ip).as_bytes())
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* SP: 0x{:x}\n", tcb.extra.sp).as_bytes())
                    .unwrap();
                report_file
                    .write_all(
                        format!("\t\t* IPC Buffer: 0x{:x}\n", tcb.extra.ipc_buffer_addr).as_bytes(),
                    )
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* Priority: {}\n", tcb.extra.prio).as_bytes())
                    .unwrap();
                report_file
                    .write_all(format!("\t\t* CPU Affinity: {}\n", tcb.extra.affinity).as_bytes())
                    .unwrap();

                report_file.write_all(b"\t\t* Bound Objects:\n").unwrap();
                for (bound_slot, cap) in tcb.slots.iter() {
                    let slot_enum = TcbBoundSlot::from(*bound_slot);
                    let object_name = &spec.get_root_object(cap.obj()).unwrap().name;
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
                        .write_all(format!("\t\t\t-> {prefix}: '{object_name}'\n").as_bytes())
                        .unwrap();
                }
            }
            _ => unreachable!("internal bug: object is not TCB!"),
        }
        report_file.write_all(b"\n").unwrap();
    }

    report_file.write_all(b"\n# CNode Details\n").unwrap();
    let cnode_objects = spec
        .objects
        .iter()
        .filter(|named_object| matches!(named_object.object, CapDLObject::CNode(_)));
    for named_cnode_object in cnode_objects {
        report_file
            .write_all(format!("\t- CNode: '{}'\n", named_cnode_object.name).as_bytes())
            .unwrap();
        for (cap_addr, cap) in named_cnode_object.object.get_cap_entries().unwrap() {
            let to_object = spec.get_root_object(cap.obj()).unwrap();
            report_file
                .write_all(format!("\t\t* Slot: {cap_addr}\n").as_bytes())
                .unwrap();

            report_file
                .write_all(format!("\t\t\t-> Object: '{}'\n", to_object.name).as_bytes())
                .unwrap();
            // @billn revisit
            // let rights_maybe = cap.rights();
            // if rights_maybe.is_some() {
            //     report_file
            //         .write_all(
            //             format!("\t\t\t-> Rights: {}\n", "")// @billn todo rights_maybe.unwrap().human_repr())
            //                 .as_bytes(),
            //         )
            //         .unwrap();
            // }
            // let badge_maybe = cap.badge();
            // if badge_maybe.is_some() {
            //     report_file
            //         .write_all(format!("\t\t\t-> Badge: 0x{:x}\n", badge_maybe.unwrap()).as_bytes())
            //         .unwrap();
            // }
        }
        report_file.write_all(b"\n").unwrap();
    }

    report_file
        .write_all(b"\n# Architecture Specific Details\n")
        .unwrap();
    match kernel_config.arch {
        Arch::Aarch64 => {
            let is_smc = spec
                .objects
                .iter()
                .filter(|named_object| matches!(named_object.object, CapDLObject::ArmSmc))
                .count()
                > 0;
            if is_smc {
                report_file
                    .write_all(b"ARM SMC access is granted to userspace.\n")
                    .unwrap();
            }
        }
        Arch::X86_64 => {
            let ioports = spec
                .objects
                .iter()
                .filter(|named_object| matches!(named_object.object, CapDLObject::IOPorts(_)));

            for named_ioport_object in ioports {
                report_file
                    .write_all(
                        format!(
                            "\t- {}: '{}'\n",
                            named_ioport_object.object.human_name(kernel_config),
                            named_ioport_object.name
                        )
                        .as_bytes(),
                    )
                    .unwrap();

                match &named_ioport_object.object {
                    CapDLObject::IOPorts(ioports) => {
                        report_file
                            .write_all(
                                format!("\t\t* Start Port: 0x{:x}\n", ioports.start_port)
                                    .as_bytes(),
                            )
                            .unwrap();
                        report_file
                            .write_all(
                                format!("\t\t* End Port: 0x{:x}\n", ioports.end_port).as_bytes(),
                            )
                            .unwrap();
                    }
                    _ => unreachable!("internal bug: object is not x86 I/O ports!"),
                }
            }
        }
        Arch::Riscv64 => {}
    }

    report_file
        .write_all(b"\n# Kernel Objects Details: ID, Type, Name, Physical Address (on ARM and RISC-V only)\n")
        .unwrap();
    for (id, named_object) in spec.objects.iter().enumerate() {
        if named_object.object.physical_size_bits(kernel_config) > 0 {
            if kernel_config.arch == Arch::X86_64 {
                report_file
                    .write_all(
                        format!(
                            "\t{} - {}: '{}'\n",
                            id,
                            named_object.object.human_name(kernel_config),
                            named_object.name,
                        )
                        .as_bytes(),
                    )
                    .unwrap();
            } else {
                match &named_object.expected_alloc {
                    Some(allocation_details) => {
                        report_file
                            .write_all(
                                format!(
                                    "\t{} - {}: '{}' @ 0x{:0>12x}\n",
                                    id,
                                    named_object.object.human_name(kernel_config),
                                    named_object.name,
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
                                    named_object.object.human_name(kernel_config),
                                    named_object.name
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
