//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::{cmp::min, collections::HashMap};

use crate::{
    elf::ElfFile,
    sdf::{self, SysMemoryRegion, SystemDescription},
    sel4::{Arch, Config},
    util::{monitor_serialise_names, monitor_serialise_u64_vec},
    MAX_PDS, MAX_VMS, PD_MAX_NAME_LENGTH, VM_MAX_NAME_LENGTH,
};

/// Patch all the required symbols in the Monitor and children PDs according to
/// the Microkit's requirements
pub fn patch_symbols(
    kernel_config: &Config,
    pd_elf_files: &mut [ElfFile],
    monitor_elf: &mut ElfFile,
    system: &SystemDescription,
) -> Result<(), String> {
    // *********************************
    // Step 1. Write ELF symbols in the monitor.
    // *********************************
    let pd_names: Vec<String> = system
        .protection_domains
        .iter()
        .map(|pd| pd.name.clone())
        .collect();
    monitor_elf
        .write_symbol(
            "pd_names_len",
            &system.protection_domains.len().to_le_bytes(),
        )
        .unwrap();
    monitor_elf
        .write_symbol(
            "pd_names",
            &monitor_serialise_names(&pd_names, MAX_PDS, PD_MAX_NAME_LENGTH),
        )
        .unwrap();

    let vm_names: Vec<String> = system
        .protection_domains
        .iter()
        .filter(|pd| pd.virtual_machine.is_some())
        .flat_map(|pd_with_vm| {
            let vm = pd_with_vm.virtual_machine.as_ref().unwrap();
            let num_vcpus = vm.vcpus.len();
            std::iter::repeat_n(vm.name.clone(), num_vcpus)
        })
        .collect();

    let vm_names_len = match kernel_config.arch {
        Arch::Aarch64 | Arch::Riscv64 => vm_names.len(),
        // VM on x86 doesn't have a separate TCB.
        Arch::X86_64 => 0,
    };
    monitor_elf
        .write_symbol("vm_names_len", &vm_names_len.to_le_bytes())
        .unwrap();
    monitor_elf
        .write_symbol(
            "vm_names",
            &monitor_serialise_names(&vm_names, MAX_VMS, VM_MAX_NAME_LENGTH),
        )
        .unwrap();

    let mut pd_stack_bottoms: Vec<u64> = Vec::new();
    for pd in system.protection_domains.iter() {
        let cur_stack_vaddr = kernel_config.pd_stack_bottom(pd.stack_size);
        pd_stack_bottoms.push(cur_stack_vaddr);
    }
    monitor_elf
        .write_symbol(
            "pd_stack_bottom_addrs",
            &monitor_serialise_u64_vec(&pd_stack_bottoms),
        )
        .unwrap();

    // *********************************
    // Step 2. Write ELF symbols for each PD
    // *********************************
    let mut mr_name_to_desc: HashMap<&String, &SysMemoryRegion> = HashMap::new();
    for mr in system.memory_regions.iter() {
        mr_name_to_desc.insert(&mr.name, mr);
    }

    for (pd_global_idx, pd) in system.protection_domains.iter().enumerate() {
        let elf_obj = &mut pd_elf_files[pd_global_idx];

        let name = pd.name.as_bytes();
        let name_length = min(name.len(), PD_MAX_NAME_LENGTH);
        elf_obj
            .write_symbol("microkit_name", &name[..name_length])
            .unwrap();
        elf_obj
            .write_symbol("microkit_passive", &[pd.passive as u8])
            .unwrap();

        let mut notification_bits: u64 = 0;
        let mut pp_bits: u64 = 0;
        for channel in system.channels.iter() {
            if channel.end_a.pd == pd_global_idx {
                if channel.end_a.notify {
                    notification_bits |= 1 << channel.end_a.id;
                }
                if channel.end_a.pp {
                    pp_bits |= 1 << channel.end_a.id;
                }
            }
            if channel.end_b.pd == pd_global_idx {
                if channel.end_b.notify {
                    notification_bits |= 1 << channel.end_b.id;
                }
                if channel.end_b.pp {
                    pp_bits |= 1 << channel.end_b.id;
                }
            }
        }
        elf_obj
            .write_symbol("microkit_irqs", &pd.irq_bits().to_le_bytes())
            .unwrap();
        elf_obj
            .write_symbol("microkit_notifications", &notification_bits.to_le_bytes())
            .unwrap();
        elf_obj
            .write_symbol("microkit_pps", &pp_bits.to_le_bytes())
            .unwrap();
        elf_obj
            .write_symbol("microkit_ioports", &pd.ioport_bits().to_le_bytes())
            .unwrap();

        let mut symbols_to_write: Vec<(&String, u64)> = Vec::new();
        for setvar in pd.setvars.iter() {
            // Check that the symbol exists in the ELF
            match elf_obj.find_symbol(&setvar.symbol) {
                Ok(sym_info) => {
                    // Sanity check that the symbol is of word size so we dont overwrite anything.
                    let expected_symbol_size = kernel_config.word_size / 8;
                    if sym_info.1 != expected_symbol_size {
                        return Err(format!(
                            "setvar to non-word size symbol '{}' for PD '{}', symbol has size '{}' bytes, expected size '{}' bytes",
                            setvar.symbol, pd.name, sym_info.1, expected_symbol_size
                        ));
                    }
                    let data = match &setvar.kind {
                        sdf::SysSetVarKind::Size { mr } => mr_name_to_desc.get(mr).unwrap().size,
                        sdf::SysSetVarKind::Vaddr { address } => *address,
                        sdf::SysSetVarKind::Paddr { region } => {
                            match mr_name_to_desc.get(region).unwrap().phys_addr {
                                Some(specified_paddr) => specified_paddr,
                                None => {
                                    if kernel_config.arch == Arch::X86_64 {
                                        return Err(
                                            "setvar with 'region_paddr' for MR without a specified paddr is unsupported on x86_64."
                                                .to_string(),
                                        );
                                    } else {
                                        panic!("setvar with 'region_paddr' for MR without a specified paddr is currently unimplemented!");
                                    }
                                }
                            }
                        }
                        sdf::SysSetVarKind::Id { id } => *id,
                    };
                    symbols_to_write.push((&setvar.symbol, data));
                }
                Err(err) => {
                    return Err(format!(
                        "could not patch symbol '{}' in program image for PD '{}' ({}): {}",
                        setvar.symbol,
                        pd.name,
                        pd.program_image.display(),
                        err
                    ))
                }
            }
        }
        let elf_obj = pd_elf_files.get_mut(pd_global_idx).unwrap();
        for (sym_name, value) in symbols_to_write.iter() {
            elf_obj
                .write_symbol(sym_name, &value.to_le_bytes())
                .unwrap();
        }
    }

    Ok(())
}
