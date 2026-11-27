//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::{cmp::min, collections::BTreeMap, rc::Rc};

use crate::{
    elf::ElfFile,
    sdf::{self, SysMemoryRegion, SystemDescription, MONITOR_PD_INFO_ADDR},
    sel4::{Arch, Config},
    util::{monitor_serialise_names, monitor_serialise_u64_vec},
    PD_MAX_NAME_LENGTH,
};

/// Patch all the required symbols in the Monitor and children PDs according to
/// the Microkit's requirements
pub fn patch_symbols(
    kernel_config: &Config,
    pd_elf_files: &mut [ElfFile],
    system: &SystemDescription,
) -> Result<(), String> {
    // *********************************
    // Step 1. Write ELF symbols in the monitor.
    // *********************************
    let monitor_elf = pd_elf_files.last_mut().unwrap();

    monitor_elf
        .write_symbol(
            "pd_infos_length",
            &system.protection_domains.len().to_le_bytes(),
        )
        .expect("symbol 'pd_infos_length' should exist in Microkit monitor");

    monitor_elf
        .write_symbol(
            "pd_infos",
            &MONITOR_PD_INFO_ADDR.to_le_bytes(),
        )
        .expect("symbol 'pd_infos_length' should exist in Microkit monitor");

    // *********************************
    // Step 2. Write ELF symbols for each PD
    // *********************************
    let mut mr_name_to_desc: BTreeMap<&str, &SysMemoryRegion> = BTreeMap::new();
    for mr in system.memory_regions.iter() {
        mr_name_to_desc.insert(&mr.name, mr);
    }

    for (pd_global_idx, pd) in system.protection_domains.values().enumerate() {
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
            if channel.end_a.pd == pd.name {
                if channel.end_a.notify {
                    notification_bits |= 1 << channel.end_a.id;
                }
                if channel.end_a.pp {
                    pp_bits |= 1 << channel.end_a.id;
                }
            }
            if channel.end_b.pd == pd.name {
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

        let mut symbols_to_write: Vec<(&str, u64)> = Vec::new();
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
                        sdf::SysSetVarKind::Size { mr } => mr_name_to_desc[mr.as_str()].size,
                        sdf::SysSetVarKind::Vaddr { address } => *address,
                        sdf::SysSetVarKind::Paddr { region } => {
                            mr_name_to_desc[region.as_str()].paddr().unwrap_or_default()
                        }
                        sdf::SysSetVarKind::Id { id } => *id,
                        sdf::SysSetVarKind::X86IoPortAddr { address } => *address,
                        sdf::SysSetVarKind::PrefillSize { mr } => mr_name_to_desc[mr.as_str()]
                            .prefill_bytes
                            .as_ref()
                            .unwrap()
                            .len()
                            as u64,
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
        let elf_obj = &mut pd_elf_files[pd_global_idx];
        for (sym_name, value) in symbols_to_write.iter() {
            elf_obj
                .write_symbol(sym_name, &value.to_le_bytes())
                .unwrap();
        }
    }

    Ok(())
}
