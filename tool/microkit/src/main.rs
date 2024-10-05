//
// Copyright 2024, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

// we want our asserts, even if the compiler figures out they hold true already during compile-time
#![allow(clippy::assertions_on_constants)]

use elf::ElfFile;
use loader::Loader;
use microkit_tool::{
    elf, loader, sdf, sel4, util, DisjointMemoryRegion, MemoryRegion, ObjectAllocator, Region,
    UntypedObject, MAX_PDS, PD_MAX_NAME_LENGTH,
};
use sdf::{
    parse, ProtectionDomain, SysMap, SysMapPerms, SysMemoryRegion, SystemDescription,
    VirtualMachine,
};
use sel4::{
    default_vm_attr, Aarch64Regs, Arch, ArmVmAttributes, BootInfo, Config, Invocation,
    InvocationArgs, Object, ObjectType, PageSize, Rights, Riscv64Regs, RiscvVirtualMemory,
    RiscvVmAttributes,
};
use std::cmp::{max, min};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufWriter, Write};
use std::iter::zip;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use util::{
    bytes_to_struct, comma_sep_u64, comma_sep_usize, json_str, json_str_as_bool, json_str_as_u64,
    struct_to_bytes,
};

// Corresponds to the IPC buffer symbol in libmicrokit and the monitor
const SYMBOL_IPC_BUFFER: &str = "__sel4_ipc_buffer_obj";

const FAULT_BADGE: u64 = 1 << 62;
const PPC_BADGE: u64 = 1 << 63;

const INPUT_CAP_IDX: u64 = 1;
#[allow(dead_code)]
const FAULT_EP_CAP_IDX: u64 = 2;
const VSPACE_CAP_IDX: u64 = 3;
const REPLY_CAP_IDX: u64 = 4;
const MONITOR_EP_CAP_IDX: u64 = 5;
const TCB_CAP_IDX: u64 = 6;
const SMC_CAP_IDX: u64 = 7;

const BASE_OUTPUT_NOTIFICATION_CAP: u64 = 10;
const BASE_OUTPUT_ENDPOINT_CAP: u64 = BASE_OUTPUT_NOTIFICATION_CAP + 64;
const BASE_IRQ_CAP: u64 = BASE_OUTPUT_ENDPOINT_CAP + 64;
const BASE_PD_TCB_CAP: u64 = BASE_IRQ_CAP + 64;
const BASE_VM_TCB_CAP: u64 = BASE_PD_TCB_CAP + 64;
const BASE_VCPU_CAP: u64 = BASE_VM_TCB_CAP + 64;

const MAX_SYSTEM_INVOCATION_SIZE: u64 = util::mb(128);

const PD_CAP_SIZE: u64 = 512;
const PD_CAP_BITS: u64 = PD_CAP_SIZE.ilog2() as u64;
const PD_SCHEDCONTEXT_SIZE: u64 = 1 << 8;

const SLOT_BITS: u64 = 5;
const SLOT_SIZE: u64 = 1 << SLOT_BITS;

const INIT_NULL_CAP_ADDRESS: u64 = 0;
const INIT_TCB_CAP_ADDRESS: u64 = 1;
const INIT_CNODE_CAP_ADDRESS: u64 = 2;
const INIT_VSPACE_CAP_ADDRESS: u64 = 3;
const IRQ_CONTROL_CAP_ADDRESS: u64 = 4; // Singleton
const INIT_ASID_POOL_CAP_ADDRESS: u64 = 6;
const SMC_CAP_ADDRESS: u64 = 15;

// const ASID_CONTROL_CAP_ADDRESS: u64 = 5; // Singleton
// const IO_PORT_CONTROL_CAP_ADDRESS: u64 = 7; // Null on this platform
// const IO_SPACE_CAP_ADDRESS: u64 = 8;  // Null on this platform
// const BOOT_INFO_FRAME_CAP_ADDRESS: u64 = 9;
// const INIT_THREAD_IPC_BUFFER_CAP_ADDRESS: u64 = 10;
// const DOMAIN_CAP_ADDRESS: u64 = 11;
// const SMMU_SID_CONTROL_CAP_ADDRESS: u64 = 12;
// const SMMU_CB_CONTROL_CAP_ADDRESS: u64 = 13;
// const INIT_THREAD_SC_CAP_ADDRESS: u64 = 14;

/// Corresponds to 'struct untyped_info' in the monitor
/// It should be noted that this is called a 'header' since
/// it omits the 'regions' field.
/// This struct assumes a 64-bit target
#[repr(C)]
struct MonitorUntypedInfoHeader64 {
    cap_start: u64,
    cap_end: u64,
}

/// Corresponds to 'struct region' in the monitor
/// This struct assumes a 64-bit target
#[repr(C)]
struct MonitorRegion64 {
    paddr: u64,
    size_bits: u64,
    is_device: u64,
}

struct MonitorConfig {
    untyped_info_symbol_name: &'static str,
    bootstrap_invocation_count_symbol_name: &'static str,
    bootstrap_invocation_data_symbol_name: &'static str,
    system_invocation_count_symbol_name: &'static str,
}

impl MonitorConfig {
    pub fn max_untyped_objects(&self, symbol_size: u64) -> u64 {
        (symbol_size - size_of::<MonitorUntypedInfoHeader64>() as u64)
            / size_of::<MonitorRegion64>() as u64
    }
}

#[derive(Debug)]
struct FixedUntypedAlloc {
    ut: UntypedObject,
    watermark: u64,
}

impl FixedUntypedAlloc {
    pub fn new(ut: UntypedObject) -> FixedUntypedAlloc {
        FixedUntypedAlloc {
            ut,
            watermark: ut.base(),
        }
    }

    pub fn contains(&self, addr: u64) -> bool {
        self.ut.base() <= addr && addr < self.ut.end()
    }
}

struct InitSystem<'a> {
    config: &'a Config,
    cnode_cap: u64,
    cnode_mask: u64,
    kao: &'a mut ObjectAllocator,
    invocations: &'a mut Vec<Invocation>,
    cap_slot: u64,
    last_fixed_address: u64,
    normal_untyped: Vec<FixedUntypedAlloc>,
    device_untyped: Vec<FixedUntypedAlloc>,
    cap_address_names: &'a mut HashMap<u64, String>,
    objects: Vec<Object>,
}

impl<'a> InitSystem<'a> {
    #[allow(clippy::too_many_arguments)] // just this one time, pinky-promise...
    pub fn new(
        config: &'a Config,
        cnode_cap: u64,
        cnode_mask: u64,
        first_available_cap_slot: u64,
        kernel_object_allocator: &'a mut ObjectAllocator,
        kernel_boot_info: &'a BootInfo,
        invocations: &'a mut Vec<Invocation>,
        cap_address_names: &'a mut HashMap<u64, String>,
    ) -> InitSystem<'a> {
        let mut device_untyped: Vec<FixedUntypedAlloc> = kernel_boot_info
            .untyped_objects
            .iter()
            .filter_map(|ut| {
                if ut.is_device {
                    Some(FixedUntypedAlloc::new(*ut))
                } else {
                    None
                }
            })
            .collect();
        device_untyped.sort_by(|a, b| a.ut.base().cmp(&b.ut.base()));

        let mut normal_untyped: Vec<FixedUntypedAlloc> = kernel_boot_info
            .untyped_objects
            .iter()
            .filter_map(|ut| {
                if !ut.is_device {
                    Some(FixedUntypedAlloc::new(*ut))
                } else {
                    None
                }
            })
            .collect();
        normal_untyped.sort_by(|a, b| a.ut.base().cmp(&b.ut.base()));

        InitSystem {
            config,
            cnode_cap,
            cnode_mask,
            kao: kernel_object_allocator,
            invocations,
            cap_slot: first_available_cap_slot,
            last_fixed_address: 0,
            normal_untyped,
            device_untyped,
            cap_address_names,
            objects: Vec::new(),
        }
    }

    pub fn reserve(&mut self, allocations: Vec<(&UntypedObject, u64)>) {
        for (alloc_ut, alloc_phys_addr) in allocations {
            let mut found = false;
            for fut in &mut self.device_untyped {
                if *alloc_ut == fut.ut {
                    if fut.ut.base() <= alloc_phys_addr && alloc_phys_addr <= fut.ut.end() {
                        fut.watermark = alloc_phys_addr;
                        found = true;
                        break;
                    } else {
                        panic!(
                            "Allocation {:?} ({:x}) not in untyped region {:?}",
                            alloc_ut, alloc_phys_addr, fut.ut.region
                        );
                    }
                }
            }

            if !found {
                panic!(
                    "Allocation {:?} ({:x}) not in any device untyped",
                    alloc_ut, alloc_phys_addr
                );
            }
        }
    }

    /// Note: Fixed objects must be allocated in order!
    pub fn allocate_fixed_object(
        &mut self,
        phys_address: u64,
        object_type: ObjectType,
        name: String,
    ) -> Object {
        assert!(phys_address >= self.last_fixed_address);
        assert!(object_type.fixed_size(self.config).is_some());

        let alloc_size = object_type.fixed_size(self.config).unwrap();
        // Find an untyped that contains the given address, it may be in device
        // memory
        let device_fut: Option<&mut FixedUntypedAlloc> = self
            .device_untyped
            .iter_mut()
            .find(|fut| fut.contains(phys_address));

        let normal_fut: Option<&mut FixedUntypedAlloc> = self
            .normal_untyped
            .iter_mut()
            .find(|fut| fut.contains(phys_address));

        // We should never have found the physical address in both device and normal untyped
        assert!(!(device_fut.is_some() && normal_fut.is_some()));

        let fut = if let Some(fut) = device_fut {
            fut
        } else if let Some(fut) = normal_fut {
            fut
        } else {
            panic!(
                "Error: physical address {:x} not in any device untyped",
                phys_address
            )
        };

        let space_left = fut.ut.region.end - fut.watermark;
        if space_left < alloc_size {
            for ut in &self.device_untyped {
                let space_left = ut.ut.region.end - ut.watermark;
                println!(
                    "ut [0x{:x}..0x{:x}], space left: 0x{:x}",
                    ut.ut.region.base, ut.ut.region.end, space_left
                );
            }
            panic!(
                "Error: allocation for physical address {:x} is too large ({:x}) for untyped",
                phys_address, alloc_size
            );
        }

        if phys_address < fut.watermark {
            panic!(
                "Error: physical address {:x} is below watermark",
                phys_address
            );
        }

        if fut.watermark != phys_address {
            // If the watermark isn't at the right spot, then we need to
            // create padding objects until it is.
            let mut padding_required = phys_address - fut.watermark;
            // We are restricted in how much we can pad:
            // 1: Untyped objects must be power-of-two sized.
            // 2: Untyped objects must be aligned to their size.
            let mut padding_sizes = Vec::new();
            // We have two potential approaches for how we pad.
            // 1: Use largest objects possible respecting alignment
            // and size restrictions.
            // 2: Use a fixed size object multiple times. This will
            // create more objects, but as same sized objects can be
            // create in a batch, required fewer invocations.
            // For now we choose #1
            let mut wm = fut.watermark;
            while padding_required > 0 {
                let wm_lsb = util::lsb(wm);
                let sz_msb = util::msb(padding_required);
                let pad_obejct_size = 1 << min(wm_lsb, sz_msb);
                padding_sizes.push(pad_obejct_size);
                wm += pad_obejct_size;
                padding_required -= pad_obejct_size;
            }

            for sz in padding_sizes {
                self.invocations.push(Invocation::new(
                    self.config,
                    InvocationArgs::UntypedRetype {
                        untyped: fut.ut.cap,
                        object_type: ObjectType::Untyped,
                        size_bits: sz.ilog2() as u64,
                        root: self.cnode_cap,
                        node_index: 1,
                        node_depth: 1,
                        node_offset: self.cap_slot,
                        num_objects: 1,
                    },
                ));
                self.cap_slot += 1;
            }
        }

        let object_cap = self.cap_slot;
        self.cap_slot += 1;
        self.invocations.push(Invocation::new(
            self.config,
            InvocationArgs::UntypedRetype {
                untyped: fut.ut.cap,
                object_type,
                size_bits: 0,
                root: self.cnode_cap,
                node_index: 1,
                node_depth: 1,
                node_offset: object_cap,
                num_objects: 1,
            },
        ));

        fut.watermark = phys_address + alloc_size;
        self.last_fixed_address = phys_address + alloc_size;
        let cap_addr = self.cnode_mask | object_cap;
        let kernel_object = Object {
            object_type,
            cap_addr,
            phys_addr: phys_address,
        };
        self.objects.push(kernel_object);
        self.cap_address_names.insert(cap_addr, name);

        kernel_object
    }

    pub fn allocate_objects(
        &mut self,
        object_type: ObjectType,
        names: Vec<String>,
        size: Option<u64>,
    ) -> Vec<Object> {
        // Nothing to do if we get a zero count.
        if names.is_empty() {
            return Vec::new();
        }

        let count = names.len() as u64;

        let alloc_size;
        let api_size: u64;
        if let Some(object_size) = object_type.fixed_size(self.config) {
            // An object with a fixed size should not be allocated with a given size
            assert!(size.is_none());
            alloc_size = object_size;
            api_size = 0;
        } else if object_type == ObjectType::CNode || object_type == ObjectType::SchedContext {
            let sz = size.unwrap();
            assert!(util::is_power_of_two(sz));
            api_size = sz.ilog2() as u64;
            alloc_size = sz * SLOT_SIZE;
        } else {
            panic!("Internal error: invalid object type: {:?}", object_type);
        }

        let allocation = self.kao.alloc_n(alloc_size, count);
        let base_cap_slot = self.cap_slot;
        self.cap_slot += count;

        let mut to_alloc = count;
        let mut alloc_cap_slot = base_cap_slot;
        while to_alloc > 0 {
            let call_count = min(to_alloc, self.config.fan_out_limit);
            self.invocations.push(Invocation::new(
                self.config,
                InvocationArgs::UntypedRetype {
                    untyped: allocation.untyped_cap_address,
                    object_type,
                    size_bits: api_size,
                    root: self.cnode_cap,
                    node_index: 1,
                    node_depth: 1,
                    node_offset: alloc_cap_slot,
                    num_objects: call_count,
                },
            ));
            to_alloc -= call_count;
            alloc_cap_slot += call_count;
        }

        let mut kernel_objects = Vec::new();
        let mut phys_addr = allocation.phys_addr;
        for (idx, name) in names.into_iter().enumerate() {
            let cap_slot = base_cap_slot + idx as u64;
            let cap_addr = self.cnode_mask | cap_slot;
            let kernel_object = Object {
                object_type,
                cap_addr,
                phys_addr,
            };
            kernel_objects.push(kernel_object);
            self.cap_address_names.insert(cap_addr, name);

            phys_addr += alloc_size;

            self.objects.push(kernel_object);
        }

        kernel_objects
    }
}

struct BuiltSystem {
    number_of_system_caps: u64,
    invocation_data: Vec<u8>,
    invocation_data_size: u64,
    bootstrap_invocations: Vec<Invocation>,
    system_invocations: Vec<Invocation>,
    kernel_boot_info: BootInfo,
    reserved_region: MemoryRegion,
    fault_ep_cap_address: u64,
    reply_cap_address: u64,
    cap_lookup: HashMap<u64, String>,
    tcb_caps: Vec<u64>,
    sched_caps: Vec<u64>,
    ntfn_caps: Vec<u64>,
    pd_elf_regions: Vec<Vec<Region>>,
    pd_setvar_values: Vec<Vec<u64>>,
    kernel_objects: Vec<Object>,
    initial_task_virt_region: MemoryRegion,
    initial_task_phys_region: MemoryRegion,
}

pub fn pd_write_symbols(
    pds: &[ProtectionDomain],
    pd_elf_files: &mut [ElfFile],
    pd_setvar_values: &[Vec<u64>],
) -> Result<(), String> {
    for (i, pd) in pds.iter().enumerate() {
        let elf = &mut pd_elf_files[i];
        let name = pd.name.as_bytes();
        let name_length = min(name.len(), PD_MAX_NAME_LENGTH);
        elf.write_symbol("microkit_name", &name[..name_length])?;
        elf.write_symbol("microkit_passive", &[pd.passive as u8])?;

        for (setvar_idx, setvar) in pd.setvars.iter().enumerate() {
            let value = pd_setvar_values[i][setvar_idx];
            let result = elf.write_symbol(&setvar.symbol, &value.to_le_bytes());
            if result.is_err() {
                return Err(format!(
                    "No symbol named '{}' in ELF '{}' for PD '{}'",
                    setvar.symbol,
                    pd.program_image.display(),
                    pd.name
                ));
            }
        }
    }

    Ok(())
}

/// Determine the physical memory regions for an ELF file with a given
/// alignment.
///
/// The returned region shall be extended (if necessary) so that the start
/// and end are congruent with the specified alignment (usually a page size).
fn phys_mem_regions_from_elf(elf: &ElfFile, alignment: u64) -> Vec<MemoryRegion> {
    assert!(alignment > 0);

    elf.segments
        .iter()
        .filter(|s| s.loadable)
        .map(|s| {
            MemoryRegion::new(
                util::round_down(s.phys_addr, alignment),
                util::round_up(s.phys_addr + s.data.len() as u64, alignment),
            )
        })
        .collect()
}

/// Determine a single physical memory region for an ELF.
///
/// Works as per phys_mem_regions_from_elf, but checks the ELF has a single
/// segment, and returns the region covering the first segment.
fn phys_mem_region_from_elf(elf: &ElfFile, alignment: u64) -> MemoryRegion {
    assert!(alignment > 0);
    assert!(elf.segments.iter().filter(|s| s.loadable).count() == 1);

    phys_mem_regions_from_elf(elf, alignment)[0]
}

/// Determine the virtual memory regions for an ELF file with a given
/// alignment.

/// The returned region shall be extended (if necessary) so that the start
/// and end are congruent with the specified alignment (usually a page size).
fn virt_mem_regions_from_elf(elf: &ElfFile, alignment: u64) -> Vec<MemoryRegion> {
    assert!(alignment > 0);
    elf.segments
        .iter()
        .filter(|s| s.loadable)
        .map(|s| {
            MemoryRegion::new(
                util::round_down(s.virt_addr, alignment),
                util::round_up(s.virt_addr + s.data.len() as u64, alignment),
            )
        })
        .collect()
}

/// Determine a single virtual memory region for an ELF.
///
/// Works as per virt_mem_regions_from_elf, but checks the ELF has a single
/// segment, and returns the region covering the first segment.
fn virt_mem_region_from_elf(elf: &ElfFile, alignment: u64) -> MemoryRegion {
    assert!(alignment > 0);
    assert!(elf.segments.iter().filter(|s| s.loadable).count() == 1);

    virt_mem_regions_from_elf(elf, alignment)[0]
}

fn get_full_path(path: &Path, search_paths: &Vec<PathBuf>) -> Option<PathBuf> {
    for search_path in search_paths {
        let full_path = search_path.join(path);
        if full_path.exists() {
            return Some(full_path.to_path_buf());
        }
    }

    None
}

struct KernelPartialBootInfo {
    device_memory: DisjointMemoryRegion,
    normal_memory: DisjointMemoryRegion,
    boot_region: MemoryRegion,
}

// Corresponds to kernel_frame_t in the kernel
#[repr(C)]
struct KernelFrameRiscv64 {
    pub paddr: u64,
    pub pptr: u64,
    pub user_accessible: i32,
}

#[repr(C)]
struct KernelFrameAarch64 {
    pub paddr: u64,
    pub pptr: u64,
    pub execute_never: i32,
    pub user_accessible: i32,
}

fn kernel_device_addrs(config: &Config, kernel_elf: &ElfFile) -> Vec<u64> {
    assert!(config.word_size == 64, "Unsupported word-size");

    let mut kernel_devices = Vec::new();
    let (vaddr, size) = kernel_elf
        .find_symbol("kernel_device_frames")
        .expect("Could not find 'kernel_device_frames' symbol");
    let kernel_frame_bytes = kernel_elf.get_data(vaddr, size).unwrap();
    let kernel_frame_size = match config.arch {
        Arch::Aarch64 => size_of::<KernelFrameAarch64>(),
        Arch::Riscv64 => size_of::<KernelFrameRiscv64>(),
    };
    let mut offset: usize = 0;
    while offset < size as usize {
        let (user_accessible, paddr) = unsafe {
            match config.arch {
                Arch::Aarch64 => {
                    let frame = bytes_to_struct::<KernelFrameAarch64>(
                        &kernel_frame_bytes[offset..offset + kernel_frame_size],
                    );
                    (frame.user_accessible, frame.paddr)
                }
                Arch::Riscv64 => {
                    let frame = bytes_to_struct::<KernelFrameRiscv64>(
                        &kernel_frame_bytes[offset..offset + kernel_frame_size],
                    );
                    (frame.user_accessible, frame.paddr)
                }
            }
        };
        if user_accessible == 0 {
            kernel_devices.push(paddr);
        }
        offset += kernel_frame_size;
    }

    kernel_devices
}

// Corresponds to p_region_t in the kernel
#[repr(C)]
struct KernelRegion64 {
    start: u64,
    end: u64,
}

fn kernel_phys_mem(kernel_config: &Config, kernel_elf: &ElfFile) -> Vec<(u64, u64)> {
    assert!(kernel_config.word_size == 64, "Unsupported word-size");
    let mut phys_mem = Vec::new();
    let (vaddr, size) = kernel_elf
        .find_symbol("avail_p_regs")
        .expect("Could not find 'avail_p_regs' symbol");
    let p_region_bytes = kernel_elf.get_data(vaddr, size).unwrap();
    let p_region_size = size_of::<KernelRegion64>();
    let mut offset: usize = 0;
    while offset < size as usize {
        let p_region = unsafe {
            bytes_to_struct::<KernelRegion64>(&p_region_bytes[offset..offset + p_region_size])
        };
        phys_mem.push((p_region.start, p_region.end));
        offset += p_region_size;
    }

    phys_mem
}

fn kernel_self_mem(kernel_elf: &ElfFile) -> MemoryRegion {
    let segments = kernel_elf.loadable_segments();
    let base = segments[0].phys_addr;
    let (ki_end_v, _) = kernel_elf
        .find_symbol("ki_end")
        .expect("Could not find 'ki_end' symbol");
    let ki_end_p = ki_end_v - segments[0].virt_addr + base;

    MemoryRegion::new(base, ki_end_p)
}

fn kernel_boot_mem(kernel_elf: &ElfFile) -> MemoryRegion {
    let segments = kernel_elf.loadable_segments();
    let base = segments[0].phys_addr;
    let (ki_boot_end_v, _) = kernel_elf
        .find_symbol("ki_boot_end")
        .expect("Could not find 'ki_boot_end' symbol");
    let ki_boot_end_p = ki_boot_end_v - segments[0].virt_addr + base;

    MemoryRegion::new(base, ki_boot_end_p)
}

///
/// Emulate what happens during a kernel boot, up to the point
/// where the reserved region is allocated.
///
/// This factors the common parts of 'emulate_kernel_boot' and
/// 'emulate_kernel_boot_partial' to avoid code duplication.
///
fn kernel_partial_boot(kernel_config: &Config, kernel_elf: &ElfFile) -> KernelPartialBootInfo {
    // Determine the untyped caps of the system
    // This lets allocations happen correctly.
    let mut device_memory = DisjointMemoryRegion::default();
    let mut normal_memory = DisjointMemoryRegion::default();

    // Start by allocating the entire physical address space
    // as device memory.
    device_memory.insert_region(0, kernel_config.paddr_user_device_top);

    // Next, remove all the kernel devices.
    // NOTE: There is an assumption each kernel device is one frame
    // in size only. It's possible this assumption could break in the
    // future.
    for paddr in kernel_device_addrs(kernel_config, kernel_elf) {
        device_memory.remove_region(paddr, paddr + kernel_config.kernel_frame_size);
    }

    // Remove all the actual physical memory from the device regions
    // but add it all to the actual normal memory regions
    for (start, end) in kernel_phys_mem(kernel_config, kernel_elf) {
        device_memory.remove_region(start, end);
        normal_memory.insert_region(start, end);
    }

    // Remove the kernel image itself
    let self_mem = kernel_self_mem(kernel_elf);
    normal_memory.remove_region(self_mem.base, self_mem.end);

    // but get the boot region, we'll add that back later
    // FIXME: Why calcaultae it now if we add it back later?
    let boot_region = kernel_boot_mem(kernel_elf);

    KernelPartialBootInfo {
        device_memory,
        normal_memory,
        boot_region,
    }
}

fn emulate_kernel_boot_partial(
    kernel_config: &Config,
    kernel_elf: &ElfFile,
) -> (DisjointMemoryRegion, MemoryRegion) {
    let partial_info = kernel_partial_boot(kernel_config, kernel_elf);
    (partial_info.normal_memory, partial_info.boot_region)
}

fn get_n_paging(region: MemoryRegion, bits: u64) -> u64 {
    let start = util::round_down(region.base, 1 << bits);
    let end = util::round_up(region.end, 1 << bits);

    (end - start) / (1 << bits)
}

fn get_arch_n_paging(config: &Config, region: MemoryRegion) -> u64 {
    match config.arch {
        Arch::Aarch64 => {
            const PT_INDEX_OFFSET: u64 = 12;
            const PD_INDEX_OFFSET: u64 = PT_INDEX_OFFSET + 9;
            const PUD_INDEX_OFFSET: u64 = PD_INDEX_OFFSET + 9;

            get_n_paging(region, PUD_INDEX_OFFSET) + get_n_paging(region, PD_INDEX_OFFSET)
        }
        Arch::Riscv64 => match config.riscv_pt_levels.unwrap() {
            RiscvVirtualMemory::Sv39 => {
                const PT_INDEX_OFFSET: u64 = 12;
                const LVL1_INDEX_OFFSET: u64 = PT_INDEX_OFFSET + 9;
                const LVL2_INDEX_OFFSET: u64 = LVL1_INDEX_OFFSET + 9;

                get_n_paging(region, LVL2_INDEX_OFFSET) + get_n_paging(region, LVL1_INDEX_OFFSET)
            }
        },
    }
}

fn rootserver_max_size_bits(config: &Config) -> u64 {
    let slot_bits = 5; // seL4_SlotBits
    let root_cnode_bits = config.init_cnode_bits; // CONFIG_ROOT_CNODE_SIZE_BITS
    let vspace_bits = ObjectType::VSpace.fixed_size_bits(config).unwrap();

    let cnode_size_bits = root_cnode_bits + slot_bits;
    max(cnode_size_bits, vspace_bits)
}

fn calculate_rootserver_size(config: &Config, initial_task_region: MemoryRegion) -> u64 {
    // FIXME: These constants should ideally come from the config / kernel
    // binary not be hard coded here.
    // But they are constant so it isn't too bad.
    let slot_bits = 5; // seL4_SlotBits
    let root_cnode_bits = config.init_cnode_bits; // CONFIG_ROOT_CNODE_SIZE_BITS
    let tcb_bits = ObjectType::Tcb.fixed_size_bits(config).unwrap(); // seL4_TCBBits
    let page_bits = ObjectType::SmallPage.fixed_size_bits(config).unwrap(); // seL4_PageBits
    let asid_pool_bits = 12; // seL4_ASIDPoolBits
    let vspace_bits = ObjectType::VSpace.fixed_size_bits(config).unwrap(); // seL4_VSpaceBits
    let page_table_bits = ObjectType::PageTable.fixed_size_bits(config).unwrap(); // seL4_PageTableBits
    let min_sched_context_bits = 7; // seL4_MinSchedContextBits

    let mut size = 0;
    size += 1 << (root_cnode_bits + slot_bits);
    size += 1 << (tcb_bits);
    size += 2 * (1 << page_bits);
    size += 1 << asid_pool_bits;
    size += 1 << vspace_bits;
    size += get_arch_n_paging(config, initial_task_region) * (1 << page_table_bits);
    size += 1 << min_sched_context_bits;

    size
}

/// Emulate what happens during a kernel boot, generating a
/// representation of the BootInfo struct.
fn emulate_kernel_boot(
    config: &Config,
    kernel_elf: &ElfFile,
    initial_task_phys_region: MemoryRegion,
    initial_task_virt_region: MemoryRegion,
    reserved_region: MemoryRegion,
) -> BootInfo {
    assert!(initial_task_phys_region.size() == initial_task_virt_region.size());
    let partial_info = kernel_partial_boot(config, kernel_elf);
    let mut normal_memory = partial_info.normal_memory;
    let device_memory = partial_info.device_memory;
    let boot_region = partial_info.boot_region;

    normal_memory.remove_region(initial_task_phys_region.base, initial_task_phys_region.end);
    normal_memory.remove_region(reserved_region.base, reserved_region.end);

    // Now, the tricky part! determine which memory is used for the initial task objects
    let initial_objects_size = calculate_rootserver_size(config, initial_task_virt_region);
    let initial_objects_align = rootserver_max_size_bits(config);

    // Find an appropriate region of normal memory to allocate the objects
    // from; this follows the same algorithm used within the kernel boot code
    // (or at least we hope it does!)
    // TOOD: this loop could be done better in a functional way?
    let mut region_to_remove: Option<u64> = None;
    for region in normal_memory.regions.iter().rev() {
        let start = util::round_down(
            region.end - initial_objects_size,
            1 << initial_objects_align,
        );
        if start >= region.base {
            region_to_remove = Some(start);
            break;
        }
    }
    if let Some(start) = region_to_remove {
        normal_memory.remove_region(start, start + initial_objects_size);
    } else {
        panic!("Couldn't find appropriate region for initial task kernel objects");
    }

    let fixed_cap_count = 0x10;
    let sched_control_cap_count = 1;
    let paging_cap_count = get_arch_n_paging(config, initial_task_virt_region);
    let page_cap_count = initial_task_virt_region.size() / config.minimum_page_size;
    let first_untyped_cap =
        fixed_cap_count + paging_cap_count + sched_control_cap_count + page_cap_count;
    let sched_control_cap = fixed_cap_count + paging_cap_count;

    let max_bits = match config.arch {
        Arch::Aarch64 => 47,
        Arch::Riscv64 => 38,
    };
    let device_regions: Vec<MemoryRegion> = [
        reserved_region.aligned_power_of_two_regions(max_bits),
        device_memory.aligned_power_of_two_regions(max_bits),
    ]
    .concat();
    let normal_regions: Vec<MemoryRegion> = [
        boot_region.aligned_power_of_two_regions(max_bits),
        normal_memory.aligned_power_of_two_regions(max_bits),
    ]
    .concat();
    let mut untyped_objects = Vec::new();
    for (i, r) in device_regions.iter().enumerate() {
        let cap = i as u64 + first_untyped_cap;
        untyped_objects.push(UntypedObject::new(cap, *r, true));
    }
    let normal_regions_start_cap = first_untyped_cap + device_regions.len() as u64;
    for (i, r) in normal_regions.iter().enumerate() {
        let cap = i as u64 + normal_regions_start_cap;
        untyped_objects.push(UntypedObject::new(cap, *r, false));
    }

    let first_available_cap =
        first_untyped_cap + device_regions.len() as u64 + normal_regions.len() as u64;
    BootInfo {
        fixed_cap_count,
        paging_cap_count,
        page_cap_count,
        sched_control_cap,
        first_available_cap,
        untyped_objects,
    }
}

fn build_system(
    config: &Config,
    pd_elf_files: &Vec<ElfFile>,
    kernel_elf: &ElfFile,
    monitor_elf: &ElfFile,
    system: &SystemDescription,
    invocation_table_size: u64,
    system_cnode_size: u64,
) -> Result<BuiltSystem, String> {
    assert!(util::is_power_of_two(system_cnode_size));
    assert!(invocation_table_size % config.minimum_page_size == 0);
    assert!(invocation_table_size <= MAX_SYSTEM_INVOCATION_SIZE);

    let mut cap_address_names: HashMap<u64, String> = HashMap::new();
    cap_address_names.insert(INIT_NULL_CAP_ADDRESS, "null".to_string());
    cap_address_names.insert(INIT_TCB_CAP_ADDRESS, "TCB: init".to_string());
    cap_address_names.insert(INIT_CNODE_CAP_ADDRESS, "CNode: init".to_string());
    cap_address_names.insert(INIT_VSPACE_CAP_ADDRESS, "VSpace: init".to_string());
    cap_address_names.insert(INIT_ASID_POOL_CAP_ADDRESS, "ASID Pool: init".to_string());
    cap_address_names.insert(IRQ_CONTROL_CAP_ADDRESS, "IRQ Control".to_string());
    cap_address_names.insert(SMC_CAP_IDX, "SMC".to_string());

    let system_cnode_bits = system_cnode_size.ilog2() as u64;

    // Emulate kernel boot

    // Determine physical memory region used by the monitor
    let initial_task_size = phys_mem_region_from_elf(monitor_elf, config.minimum_page_size).size();

    // Determine physical memory region for 'reserved' memory.
    //
    // The 'reserved' memory region will not be touched by seL4 during boot
    // and allows the monitor (initial task) to create memory regions
    // from this area, which can then be made available to the appropriate
    // protection domains
    let mut pd_elf_size = 0;
    for pd_elf in pd_elf_files {
        for r in phys_mem_regions_from_elf(pd_elf, config.minimum_page_size) {
            pd_elf_size += r.size();
        }
    }
    let reserved_size = invocation_table_size + pd_elf_size;

    // Now that the size is determined, find a free region in the physical memory
    // space.
    let (mut available_memory, kernel_boot_region) =
        emulate_kernel_boot_partial(config, kernel_elf);

    // The kernel relies on the reserved region being allocated above the kernel
    // boot/ELF region, so we have the end of the kernel boot region as the lower
    // bound for allocating the reserved region.
    let reserved_base = available_memory.allocate_from(reserved_size, kernel_boot_region.end);
    assert!(kernel_boot_region.base < reserved_base);
    // The kernel relies on the initial task being allocated above the reserved
    // region, so we have the address of the end of the reserved region as the
    // lower bound for allocating the initial task.
    let initial_task_phys_base =
        available_memory.allocate_from(initial_task_size, reserved_base + reserved_size);
    assert!(reserved_base < initial_task_phys_base);

    let initial_task_phys_region = MemoryRegion::new(
        initial_task_phys_base,
        initial_task_phys_base + initial_task_size,
    );
    let initial_task_virt_region = virt_mem_region_from_elf(monitor_elf, config.minimum_page_size);

    let reserved_region = MemoryRegion::new(reserved_base, reserved_base + reserved_size);

    // Now that the reserved region has been allocated we can determine the specific
    // region of physical memory required for the inovcation table itself, and
    // all the ELF segments
    let invocation_table_region =
        MemoryRegion::new(reserved_base, reserved_base + invocation_table_size);

    // 1.3 With both the initial task region and reserved region determined the kernel
    // boot can be emulated. This provides the boot info information which is needed
    // for the next steps
    let kernel_boot_info = emulate_kernel_boot(
        config,
        kernel_elf,
        initial_task_phys_region,
        initial_task_virt_region,
        reserved_region,
    );

    for ut in &kernel_boot_info.untyped_objects {
        let dev_str = if ut.is_device { " (device)" } else { "" };
        let ut_str = format!(
            "Untyped @ 0x{:x}:0x{:x}{}",
            ut.region.base,
            ut.region.size(),
            dev_str
        );
        cap_address_names.insert(ut.cap, ut_str);
    }

    // The kernel boot info allows us to create an allocator for kernel objects
    let mut kao = ObjectAllocator::new(&kernel_boot_info);

    // 2. Now that the available resources are known it is possible to proceed with the
    // monitor task boot strap.
    //
    // The boot strap of the monitor works in two phases:
    //
    //   1. Setting up the monitor's CSpace
    //   2. Making the system invocation table available in the monitor's address
    //   space.

    // 2.1 The monitor's CSpace consists of two CNodes: a/ the initial task CNode
    // which consists of all the fixed initial caps along with caps for the
    // object create during kernel bootstrap, and b/ the system CNode, which
    // contains caps to all objects that will be created in this process.
    // The system CNode is of `system_cnode_size`. (Note: see also description
    // on how `system_cnode_size` is iteratively determined).
    //
    // The system CNode is not available at startup and must be created (by retyping
    // memory from an untyped object). Once created the two CNodes must be aranged
    // as a tree such that the slots in both CNodes are addressable.
    //
    // The system CNode shall become the root of the CSpace. The initial CNode shall
    // be copied to slot zero of the system CNode. In this manner all caps in the initial
    // CNode will keep their original cap addresses. This isn't required but it makes
    // allocation, debugging and reasoning about the system more straight forward.
    //
    // The guard shall be selected so the least significant bits are used. The guard
    // for the root shall be:
    //
    //   64 - system cnode bits - initial cnode bits
    //
    // The guard for the initial CNode will be zero.
    //
    // 2.1.1: Allocate the *root* CNode. It is two entries:
    //  slot 0: the existing init cnode
    //  slot 1: our main system cnode
    let root_cnode_bits = 1;
    let root_cnode_allocation = kao.alloc((1 << root_cnode_bits) * (1 << SLOT_BITS));
    let root_cnode_cap = kernel_boot_info.first_available_cap;
    cap_address_names.insert(root_cnode_cap, "CNode: root".to_string());

    // 2.1.2: Allocate the *system* CNode. It is the cnodes that
    // will have enough slots for all required caps.
    let system_cnode_allocation = kao.alloc(system_cnode_size * (1 << SLOT_BITS));
    let system_cnode_cap = kernel_boot_info.first_available_cap + 1;
    cap_address_names.insert(system_cnode_cap, "CNode: system".to_string());

    // 2.1.3: Now that we've allocated the space for these we generate
    // the actual systems calls.
    //
    // First up create the root cnode
    let mut bootstrap_invocations = Vec::new();

    bootstrap_invocations.push(Invocation::new(
        config,
        InvocationArgs::UntypedRetype {
            untyped: root_cnode_allocation.untyped_cap_address,
            object_type: ObjectType::CNode,
            size_bits: root_cnode_bits,
            root: INIT_CNODE_CAP_ADDRESS,
            node_index: 0,
            node_depth: 0,
            node_offset: root_cnode_cap,
            num_objects: 1,
        },
    ));

    // 2.1.4: Now insert a cap to the initial Cnode into slot zero of the newly
    // allocated root Cnode. It uses sufficient guard bits to ensure it is
    // completed padded to word size
    //
    // guard size is the lower bit of the guard, upper bits are the guard itself
    // which for out purposes is always zero.
    let guard = config.cap_address_bits - root_cnode_bits - config.init_cnode_bits;
    bootstrap_invocations.push(Invocation::new(
        config,
        InvocationArgs::CnodeMint {
            cnode: root_cnode_cap,
            dest_index: 0,
            dest_depth: root_cnode_bits,
            src_root: INIT_CNODE_CAP_ADDRESS,
            src_obj: INIT_CNODE_CAP_ADDRESS,
            src_depth: config.cap_address_bits,
            rights: Rights::All as u64,
            badge: guard,
        },
    ));

    // 2.1.5: Now it is possible to switch our root Cnode to the newly create
    // root cnode. We have a zero sized guard. This Cnode represents the top
    // bit of any cap addresses.
    let root_guard = 0;
    bootstrap_invocations.push(Invocation::new(
        config,
        InvocationArgs::TcbSetSpace {
            tcb: INIT_TCB_CAP_ADDRESS,
            fault_ep: INIT_NULL_CAP_ADDRESS,
            cspace_root: root_cnode_cap,
            cspace_root_data: root_guard,
            vspace_root: INIT_VSPACE_CAP_ADDRESS,
            vspace_root_data: 0,
        },
    ));

    // 2.1.6: Now we can create our new system Cnode. We will place it into
    // a temporary cap slot in the initial CNode to start with.
    bootstrap_invocations.push(Invocation::new(
        config,
        InvocationArgs::UntypedRetype {
            untyped: system_cnode_allocation.untyped_cap_address,
            object_type: ObjectType::CNode,
            size_bits: system_cnode_bits,
            root: INIT_CNODE_CAP_ADDRESS,
            node_index: 0,
            node_depth: 0,
            node_offset: system_cnode_cap,
            num_objects: 1,
        },
    ));

    // 2.1.7: Now that the we have create the object, we can 'mutate' it
    // to the correct place:
    // Slot #1 of the new root cnode
    let system_cap_address_mask = 1 << (config.cap_address_bits - 1);
    bootstrap_invocations.push(Invocation::new(
        config,
        InvocationArgs::CnodeMint {
            cnode: root_cnode_cap,
            dest_index: 1,
            dest_depth: root_cnode_bits,
            src_root: INIT_CNODE_CAP_ADDRESS,
            src_obj: system_cnode_cap,
            src_depth: config.cap_address_bits,
            rights: Rights::All as u64,
            badge: config.cap_address_bits - root_cnode_bits - system_cnode_bits,
        },
    ));

    // 2.2 At this point it is necessary to get the frames containing the
    // main system invocations into the virtual address space. (Remember the
    // invocations we are writing out here actually _execute_ at run time!
    // It is a bit weird that we talk about mapping in the invocation data
    // before we have even generated the invocation data!).
    //
    // This needs a few steps:
    //
    // 1. Turn untyped into page objects
    // 2. Map the page objects into the address space
    //

    // 2.2.1: The memory for the system invocation data resides at the start
    // of the reserved region. We can retype multiple frames as a time (
    // which reduces the number of invocations we need). However, it is possible
    // that the region spans multiple untyped objects.
    // At this point in time we assume we will map the area using the minimum
    // page size. It would be good in the future to use super pages (when
    // it makes sense to - this would reduce memory usage, and the number of
    // invocations required to set up the address space
    let pages_required = invocation_table_size / config.minimum_page_size;
    let base_page_cap = 0;
    for pta in base_page_cap..base_page_cap + pages_required {
        cap_address_names.insert(
            system_cap_address_mask | pta,
            "SmallPage: monitor invocation table".to_string(),
        );
    }

    let mut remaining_pages = pages_required;
    let mut invocation_table_allocations = Vec::new();
    let mut cap_slot = base_page_cap;
    let mut phys_addr = invocation_table_region.base;

    let boot_info_device_untypeds: Vec<&UntypedObject> = kernel_boot_info
        .untyped_objects
        .iter()
        .filter(|o| o.is_device)
        .collect();
    for ut in boot_info_device_untypeds {
        let ut_pages = ut.region.size() / config.minimum_page_size;
        let retype_page_count = min(ut_pages, remaining_pages);

        let mut retypes_remaining = retype_page_count;
        while retypes_remaining > 0 {
            let num_retypes = min(retypes_remaining, config.fan_out_limit);
            bootstrap_invocations.push(Invocation::new(
                config,
                InvocationArgs::UntypedRetype {
                    untyped: ut.cap,
                    object_type: ObjectType::SmallPage,
                    size_bits: 0,
                    root: root_cnode_cap,
                    node_index: 1,
                    node_depth: 1,
                    node_offset: cap_slot,
                    num_objects: num_retypes,
                },
            ));

            retypes_remaining -= num_retypes;
            cap_slot += num_retypes;
        }

        remaining_pages -= retype_page_count;
        phys_addr += retype_page_count * config.minimum_page_size;
        invocation_table_allocations.push((ut, phys_addr));
        if remaining_pages == 0 {
            break;
        }
    }

    // 2.2.1: Now that physical pages have been allocated it is possible to setup
    // the virtual memory objects so that the pages can be mapped into virtual memory
    // At this point we map into the arbitrary address of 0x0.8000.0000 (i.e.: 2GiB)
    // We arbitrary limit the maximum size to be 128MiB. This allows for at least 1 million
    // invocations to occur at system startup. This should be enough for any reasonable
    // sized system.
    //
    // Before mapping it is necessary to install page tables that can cover the region.
    let large_page_size = ObjectType::LargePage.fixed_size(config).unwrap();
    let page_table_size = ObjectType::PageTable.fixed_size(config).unwrap();
    let page_tables_required =
        util::round_up(invocation_table_size, large_page_size) / large_page_size;
    let page_table_allocation = kao.alloc_n(page_table_size, page_tables_required);
    let base_page_table_cap = cap_slot;

    for pta in base_page_table_cap..base_page_table_cap + page_tables_required {
        cap_address_names.insert(
            system_cap_address_mask | pta,
            "PageTable: monitor".to_string(),
        );
    }

    assert!(page_tables_required <= config.fan_out_limit);
    bootstrap_invocations.push(Invocation::new(
        config,
        InvocationArgs::UntypedRetype {
            untyped: page_table_allocation.untyped_cap_address,
            object_type: ObjectType::PageTable,
            size_bits: 0,
            root: root_cnode_cap,
            node_index: 1,
            node_depth: 1,
            node_offset: cap_slot,
            num_objects: page_tables_required,
        },
    ));
    cap_slot += page_tables_required;

    let page_table_vaddr: u64 = 0x8000_0000;
    // Now that the page tables are allocated they can be mapped into vspace
    let bootstrap_pt_attr = match config.arch {
        Arch::Aarch64 => ArmVmAttributes::default(),
        Arch::Riscv64 => RiscvVmAttributes::default(),
    };
    let mut pt_map_invocation = Invocation::new(
        config,
        InvocationArgs::PageTableMap {
            page_table: system_cap_address_mask | base_page_table_cap,
            vspace: INIT_VSPACE_CAP_ADDRESS,
            vaddr: page_table_vaddr,
            attr: bootstrap_pt_attr,
        },
    );
    pt_map_invocation.repeat(
        page_tables_required as u32,
        InvocationArgs::PageTableMap {
            page_table: 1,
            vspace: 0,
            vaddr: ObjectType::LargePage.fixed_size(config).unwrap(),
            attr: 0,
        },
    );
    bootstrap_invocations.push(pt_map_invocation);

    // Finally, once the page tables are allocated the pages can be mapped
    let page_vaddr: u64 = 0x8000_0000;
    let bootstrap_page_attr = match config.arch {
        Arch::Aarch64 => ArmVmAttributes::default() | ArmVmAttributes::ExecuteNever as u64,
        Arch::Riscv64 => RiscvVmAttributes::default() | RiscvVmAttributes::ExecuteNever as u64,
    };
    let mut map_invocation = Invocation::new(
        config,
        InvocationArgs::PageMap {
            page: system_cap_address_mask | base_page_cap,
            vspace: INIT_VSPACE_CAP_ADDRESS,
            vaddr: page_vaddr,
            rights: Rights::Read as u64,
            attr: bootstrap_page_attr,
        },
    );
    map_invocation.repeat(
        pages_required as u32,
        InvocationArgs::PageMap {
            page: 1,
            vspace: 0,
            vaddr: config.minimum_page_size,
            rights: 0,
            attr: 0,
        },
    );
    bootstrap_invocations.push(map_invocation);

    // 3. Now we can start setting up the system based on the information
    // the user provided in the System Description Format.
    //
    // Create all the objects:
    //
    //  TCBs: one per PD
    //  Endpoints: one per PD with a PP + one for the monitor
    //  Notification: one per PD
    //  VSpaces: one per PD
    //  CNodes: one per PD
    //  Small Pages:
    //     one per pd for IPC buffer
    //     as needed by MRs
    //  Large Pages:
    //     as needed by MRs
    //  Page table structs:
    //     as needed by protection domains based on mappings required
    let mut phys_addr_next = reserved_base + invocation_table_size;
    // Now we create additional MRs (and mappings) for the ELF files.
    let mut pd_elf_regions: Vec<Vec<Region>> = Vec::with_capacity(system.protection_domains.len());
    let mut extra_mrs = Vec::new();
    let mut pd_extra_maps: HashMap<&ProtectionDomain, Vec<SysMap>> = HashMap::new();
    for (i, pd) in system.protection_domains.iter().enumerate() {
        pd_elf_regions.push(Vec::with_capacity(pd_elf_files[i].segments.len()));
        for (seg_idx, segment) in pd_elf_files[i].segments.iter().enumerate() {
            if !segment.loadable {
                continue;
            }

            let segment_phys_addr = phys_addr_next + (segment.virt_addr % config.minimum_page_size);
            pd_elf_regions[i].push(Region::new(
                format!("PD-ELF {}-{}", pd.name, seg_idx),
                segment_phys_addr,
                segment.data.len() as u64,
                seg_idx,
            ));

            let mut perms = 0;
            if segment.is_readable() {
                perms |= SysMapPerms::Read as u8;
            }
            if segment.is_writable() {
                perms |= SysMapPerms::Write as u8;
            }
            if segment.is_executable() {
                perms |= SysMapPerms::Execute as u8;
            }

            let base_vaddr = util::round_down(segment.virt_addr, config.minimum_page_size);
            let end_vaddr = util::round_up(
                segment.virt_addr + segment.mem_size(),
                config.minimum_page_size,
            );
            let aligned_size = end_vaddr - base_vaddr;
            let name = format!("ELF:{}-{}", pd.name, seg_idx);
            let mr = SysMemoryRegion {
                name,
                size: aligned_size,
                page_size: PageSize::Small,
                page_count: aligned_size / PageSize::Small as u64,
                phys_addr: Some(phys_addr_next),
                text_pos: None,
            };
            phys_addr_next += aligned_size;

            let mp = SysMap {
                mr: mr.name.clone(),
                vaddr: base_vaddr,
                perms,
                cached: true,
                text_pos: None,
            };
            if let Some(extra_maps) = pd_extra_maps.get_mut(pd) {
                extra_maps.push(mp);
            } else {
                pd_extra_maps.insert(pd, vec![mp]);
            }

            // Add to extra_mrs at the end to avoid movement issues with the MR since it's used in
            // constructing the SysMap struct
            extra_mrs.push(mr);
        }
    }

    assert!(phys_addr_next - (reserved_base + invocation_table_size) == pd_elf_size);

    // Here we create a memory region/mapping for the stack for each PD.
    // We allocate the stack at the highest possible virtual address that the
    // kernel allows us.
    for pd in &system.protection_domains {
        let stack_mr = SysMemoryRegion {
            name: format!("STACK:{}", pd.name),
            size: pd.stack_size,
            page_size: PageSize::Small,
            page_count: pd.stack_size / PageSize::Small as u64,
            phys_addr: None,
            text_pos: None,
        };

        let stack_map = SysMap {
            mr: stack_mr.name.clone(),
            vaddr: config.pd_stack_bottom(pd.stack_size),
            perms: SysMapPerms::Read as u8 | SysMapPerms::Write as u8,
            cached: true,
            text_pos: None,
        };

        extra_mrs.push(stack_mr);
        pd_extra_maps.get_mut(pd).unwrap().push(stack_map);
    }

    let mut all_mrs: Vec<&SysMemoryRegion> =
        Vec::with_capacity(system.memory_regions.len() + extra_mrs.len());
    for mr_set in [&system.memory_regions, &extra_mrs] {
        for mr in mr_set {
            all_mrs.push(mr);
        }
    }
    let all_mr_by_name: HashMap<&str, &SysMemoryRegion> =
        all_mrs.iter().map(|mr| (mr.name.as_str(), *mr)).collect();

    let mut system_invocations: Vec<Invocation> = Vec::new();
    let mut init_system = InitSystem::new(
        config,
        root_cnode_cap,
        system_cap_address_mask,
        cap_slot,
        &mut kao,
        &kernel_boot_info,
        &mut system_invocations,
        &mut cap_address_names,
    );

    init_system.reserve(invocation_table_allocations);

    // 3.1 Work out how many regular (non-fixed) page objects are required
    let mut small_page_names = Vec::new();
    let mut large_page_names = Vec::new();

    for pd in &system.protection_domains {
        let (page_size_human, page_size_label) = util::human_size_strict(PageSize::Small as u64);
        let ipc_buffer_str = format!(
            "Page({} {}): IPC Buffer PD={}",
            page_size_human, page_size_label, pd.name
        );
        small_page_names.push(ipc_buffer_str);
    }

    for mr in &all_mrs {
        if mr.phys_addr.is_some() {
            continue;
        }

        let (page_size_human, page_size_label) = util::human_size_strict(mr.page_size as u64);
        for idx in 0..mr.page_count {
            let page_str = format!(
                "Page({} {}): MR={} #{}",
                page_size_human, page_size_label, mr.name, idx
            );
            match mr.page_size as PageSize {
                PageSize::Small => small_page_names.push(page_str),
                PageSize::Large => large_page_names.push(page_str),
            }
        }
    }

    let large_page_objs =
        init_system.allocate_objects(ObjectType::LargePage, large_page_names, None);
    let small_page_objs =
        init_system.allocate_objects(ObjectType::SmallPage, small_page_names, None);

    // All the IPC buffers are the first to be allocated which is why this works
    let ipc_buffer_objs = &small_page_objs[..system.protection_domains.len()];

    let mut mr_pages: HashMap<&SysMemoryRegion, Vec<Object>> = HashMap::new();

    let mut page_small_idx = ipc_buffer_objs.len();
    let mut page_large_idx = 0;

    for mr in &all_mrs {
        if mr.phys_addr.is_some() {
            mr_pages.insert(mr, vec![]);
            continue;
        }
        let idx = match mr.page_size {
            PageSize::Small => page_small_idx,
            PageSize::Large => page_large_idx,
        };
        let objs = match mr.page_size {
            PageSize::Small => small_page_objs[idx..idx + mr.page_count as usize].to_vec(),
            PageSize::Large => large_page_objs[idx..idx + mr.page_count as usize].to_vec(),
        };
        mr_pages.insert(mr, objs);
        match mr.page_size {
            PageSize::Small => page_small_idx += mr.page_count as usize,
            PageSize::Large => page_large_idx += mr.page_count as usize,
        }
    }

    // 3.2 Now allocate all the fixed MRs

    // First we need to find all the requested pages and sorted them
    let mut fixed_pages = Vec::new();
    for mr in &all_mrs {
        if let Some(mut phys_addr) = mr.phys_addr {
            for _ in 0..mr.page_count {
                fixed_pages.push((phys_addr, mr));
                phys_addr += mr.page_bytes();
            }
        }
    }

    // Sort based on the starting physical address
    fixed_pages.sort_by_key(|p| p.0);

    // FIXME: At this point we can recombine them into
    // groups to optimize allocation
    for (phys_addr, mr) in fixed_pages {
        let obj_type = match mr.page_size {
            PageSize::Small => ObjectType::SmallPage,
            PageSize::Large => ObjectType::LargePage,
        };

        let (page_size_human, page_size_label) = util::human_size_strict(mr.page_size as u64);
        let name = format!(
            "Page({} {}): MR={} @ {:x}",
            page_size_human, page_size_label, mr.name, phys_addr
        );
        let page = init_system.allocate_fixed_object(phys_addr, obj_type, name);
        mr_pages.get_mut(mr).unwrap().push(page);
    }

    let virtual_machines: Vec<&VirtualMachine> = system
        .protection_domains
        .iter()
        .filter_map(|pd| match &pd.virtual_machine {
            Some(vm) => Some(vm),
            None => None,
        })
        .collect();

    // TCBs
    let mut tcb_names: Vec<String> = system
        .protection_domains
        .iter()
        .map(|pd| format!("TCB: PD={}", pd.name))
        .collect();
    let mut vcpu_tcb_names = vec![];
    for vm in &virtual_machines {
        for vcpu in &vm.vcpus {
            vcpu_tcb_names.push(format!("TCB: VM(VCPU-{})={}", vcpu.id, vm.name));
        }
    }
    tcb_names.extend(vcpu_tcb_names);
    let tcb_objs = init_system.allocate_objects(ObjectType::Tcb, tcb_names, None);
    let tcb_caps: Vec<u64> = tcb_objs.iter().map(|tcb| tcb.cap_addr).collect();

    let pd_tcb_objs = &tcb_objs[..system.protection_domains.len()];
    let vcpu_tcb_objs = &tcb_objs[system.protection_domains.len()..];
    assert!(pd_tcb_objs.len() + vcpu_tcb_objs.len() == tcb_objs.len());
    // VCPUs
    let mut vcpu_names = vec![];
    for vm in &virtual_machines {
        for vcpu in &vm.vcpus {
            vcpu_names.push(format!("VCPU-{}: VM={}", vcpu.id, vm.name));
        }
    }
    let vcpu_objs = init_system.allocate_objects(ObjectType::Vcpu, vcpu_names, None);
    // Scheduling Contexts
    let mut sched_context_names: Vec<String> = system
        .protection_domains
        .iter()
        .map(|pd| format!("SchedContext: PD={}", pd.name))
        .collect();
    let mut vm_sched_context_names = vec![];
    for vm in &virtual_machines {
        for vcpu in &vm.vcpus {
            vm_sched_context_names.push(format!("SchedContext: VM(VCPU-{})={}", vcpu.id, vm.name));
        }
    }
    sched_context_names.extend(vm_sched_context_names);
    let sched_context_objs = init_system.allocate_objects(
        ObjectType::SchedContext,
        sched_context_names,
        Some(PD_SCHEDCONTEXT_SIZE),
    );
    let sched_context_caps: Vec<u64> = sched_context_objs.iter().map(|sc| sc.cap_addr).collect();

    let pd_sched_context_objs = &sched_context_objs[..system.protection_domains.len()];
    let vm_sched_context_objs = &sched_context_objs[system.protection_domains.len()..];

    // Endpoints
    let pd_endpoint_names: Vec<String> = system
        .protection_domains
        .iter()
        .filter(|pd| pd.needs_ep())
        .map(|pd| format!("EP: PD={}", pd.name))
        .collect();
    let endpoint_names = [vec![format!("EP: Monitor Fault")], pd_endpoint_names].concat();
    // Reply objects
    let pd_reply_names: Vec<String> = system
        .protection_domains
        .iter()
        .map(|pd| format!("Reply: PD={}", pd.name))
        .collect();
    let reply_names = [vec![format!("Reply: Monitor")], pd_reply_names].concat();
    let reply_objs = init_system.allocate_objects(ObjectType::Reply, reply_names, None);
    let reply_obj = &reply_objs[0];
    // FIXME: Probably only need reply objects for PPs
    let pd_reply_objs = &reply_objs[1..];
    let endpoint_objs = init_system.allocate_objects(ObjectType::Endpoint, endpoint_names, None);
    let fault_ep_endpoint_object = &endpoint_objs[0];

    // Because the first reply object is for the monitor, we map from index 1 of endpoint_objs
    let pd_endpoint_objs: Vec<Option<&Object>> = {
        let mut i = 0;
        system
            .protection_domains
            .iter()
            .map(|pd| {
                if pd.needs_ep() {
                    let obj = &endpoint_objs[1..][i];
                    i += 1;
                    Some(obj)
                } else {
                    None
                }
            })
            .collect()
    };

    let notification_names = system
        .protection_domains
        .iter()
        .map(|pd| format!("Notification: PD={}", pd.name))
        .collect();
    let notification_objs =
        init_system.allocate_objects(ObjectType::Notification, notification_names, None);
    let notification_caps = notification_objs.iter().map(|ntfn| ntfn.cap_addr).collect();

    // Determine number of upper directory / directory / page table objects required
    //
    // Upper directory (level 3 table) is based on how many 512 GiB parts of the address
    // space is covered (normally just 1!).
    //
    // Page directory (level 2 table) is based on how many 1,024 MiB parts of
    // the address space is covered
    //
    // Page table (level 3 table) is based on how many 2 MiB parts of the
    // address space is covered (excluding any 2MiB regions covered by large
    // pages).
    let mut all_pd_uds: Vec<(usize, u64)> = Vec::new();
    let mut all_pd_ds: Vec<(usize, u64)> = Vec::new();
    let mut all_pd_pts: Vec<(usize, u64)> = Vec::new();
    for (pd_idx, pd) in system.protection_domains.iter().enumerate() {
        let (ipc_buffer_vaddr, _) = pd_elf_files[pd_idx]
            .find_symbol(SYMBOL_IPC_BUFFER)
            .unwrap_or_else(|_| panic!("Could not find {}", SYMBOL_IPC_BUFFER));
        let mut upper_directory_vaddrs = HashSet::new();
        let mut directory_vaddrs = HashSet::new();
        let mut page_table_vaddrs = HashSet::new();

        // For each page, in each map determine we determine
        // which upper directory, directory and page table is resides
        // in, and then page sure this is set
        let mut vaddrs = vec![(ipc_buffer_vaddr, PageSize::Small)];
        for map_set in [&pd.maps, &pd_extra_maps[pd]] {
            for map in map_set {
                let mr = all_mr_by_name[map.mr.as_str()];
                let mut vaddr = map.vaddr;
                for _ in 0..mr.page_count {
                    vaddrs.push((vaddr, mr.page_size));
                    vaddr += mr.page_bytes();
                }
            }
        }

        for (vaddr, page_size) in vaddrs {
            match config.arch {
                Arch::Aarch64 => {
                    if !config.hypervisor && config.arm_pa_size_bits.unwrap() != 40 {
                        upper_directory_vaddrs.insert(util::mask_bits(vaddr, 12 + 9 + 9 + 9));
                    }
                }
                Arch::Riscv64 => {}
            }

            directory_vaddrs.insert(util::mask_bits(vaddr, 12 + 9 + 9));
            if page_size == PageSize::Small {
                page_table_vaddrs.insert(util::mask_bits(vaddr, 12 + 9));
            }
        }

        let mut pd_uds: Vec<(usize, u64)> = upper_directory_vaddrs
            .into_iter()
            .map(|vaddr| (pd_idx, vaddr))
            .collect();
        pd_uds.sort_by_key(|ud| ud.1);
        all_pd_uds.extend(pd_uds);

        let mut pd_ds: Vec<(usize, u64)> = directory_vaddrs
            .into_iter()
            .map(|vaddr| (pd_idx, vaddr))
            .collect();
        pd_ds.sort_by_key(|d| d.1);
        all_pd_ds.extend(pd_ds);

        let mut pd_pts: Vec<(usize, u64)> = page_table_vaddrs
            .into_iter()
            .map(|vaddr| (pd_idx, vaddr))
            .collect();
        pd_pts.sort_by_key(|pt| pt.1);
        all_pd_pts.extend(pd_pts);
    }
    all_pd_uds.sort_by_key(|ud| ud.0);
    all_pd_ds.sort_by_key(|d| d.0);
    all_pd_pts.sort_by_key(|pt| pt.0);

    let mut all_vm_uds: Vec<(usize, u64)> = Vec::new();
    let mut all_vm_ds: Vec<(usize, u64)> = Vec::new();
    let mut all_vm_pts: Vec<(usize, u64)> = Vec::new();
    for (vm_idx, vm) in virtual_machines.iter().enumerate() {
        let mut upper_directory_vaddrs = HashSet::new();
        let mut directory_vaddrs = HashSet::new();
        let mut page_table_vaddrs = HashSet::new();

        let mut vaddrs = vec![];
        for map in &vm.maps {
            let mr = all_mr_by_name[map.mr.as_str()];
            let mut vaddr = map.vaddr;
            for _ in 0..mr.page_count {
                vaddrs.push((vaddr, mr.page_size));
                vaddr += mr.page_bytes();
            }
        }

        for (vaddr, page_size) in vaddrs {
            assert!(config.hypervisor);
            if config.arm_pa_size_bits.unwrap() != 40 {
                upper_directory_vaddrs.insert(util::mask_bits(vaddr, 12 + 9 + 9 + 9));
            }
            directory_vaddrs.insert(util::mask_bits(vaddr, 12 + 9 + 9));
            if page_size == PageSize::Small {
                page_table_vaddrs.insert(util::mask_bits(vaddr, 12 + 9));
            }
        }

        let mut vm_uds: Vec<(usize, u64)> = upper_directory_vaddrs
            .into_iter()
            .map(|vaddr| (vm_idx, vaddr))
            .collect();
        vm_uds.sort_by_key(|ud| ud.1);
        all_vm_uds.extend(vm_uds);

        let mut vm_ds: Vec<(usize, u64)> = directory_vaddrs
            .into_iter()
            .map(|vaddr| (vm_idx, vaddr))
            .collect();
        vm_ds.sort_by_key(|d| d.1);
        all_vm_ds.extend(vm_ds);

        let mut vm_pts: Vec<(usize, u64)> = page_table_vaddrs
            .into_iter()
            .map(|vaddr| (vm_idx, vaddr))
            .collect();
        vm_pts.sort_by_key(|pt| pt.1);
        all_vm_pts.extend(vm_pts);
    }
    all_vm_uds.sort_by_key(|ud| ud.0);
    all_vm_ds.sort_by_key(|d| d.0);
    all_vm_pts.sort_by_key(|pt| pt.0);

    let pd_names: Vec<&str> = system
        .protection_domains
        .iter()
        .map(|pd| pd.name.as_str())
        .collect();
    let vm_names: Vec<&str> = virtual_machines.iter().map(|vm| vm.name.as_str()).collect();

    let mut vspace_names: Vec<String> = system
        .protection_domains
        .iter()
        .map(|pd| format!("VSpace: PD={}", pd.name))
        .collect();
    let vm_vspace_names: Vec<String> = virtual_machines
        .iter()
        .map(|vm| format!("VSpace: VM={}", vm.name))
        .collect();
    vspace_names.extend(vm_vspace_names);
    let vspace_objs = init_system.allocate_objects(ObjectType::VSpace, vspace_names, None);
    let pd_vspace_objs = &vspace_objs[..system.protection_domains.len()];
    let vm_vspace_objs = &vspace_objs[system.protection_domains.len()..];

    let pd_ud_names: Vec<String> = all_pd_uds
        .iter()
        .map(|(pd_idx, vaddr)| format!("PageTable: PD={} VADDR=0x{:x}", pd_names[*pd_idx], vaddr))
        .collect();
    let vm_ud_names: Vec<String> = all_vm_uds
        .iter()
        .map(|(vm_idx, vaddr)| format!("PageTable: VM={} VADDR=0x{:x}", vm_names[*vm_idx], vaddr))
        .collect();

    let pd_ud_objs = init_system.allocate_objects(ObjectType::PageTable, pd_ud_names, None);
    let vm_ud_objs = init_system.allocate_objects(ObjectType::PageTable, vm_ud_names, None);

    if !config.hypervisor {
        assert!(vm_ud_objs.is_empty());
    }

    let pd_d_names: Vec<String> = all_pd_ds
        .iter()
        .map(|(pd_idx, vaddr)| format!("PageTable: PD={} VADDR=0x{:x}", pd_names[*pd_idx], vaddr))
        .collect();
    let vm_d_names: Vec<String> = all_vm_ds
        .iter()
        .map(|(vm_idx, vaddr)| format!("PageTable: VM={} VADDR=0x{:x}", vm_names[*vm_idx], vaddr))
        .collect();
    let pd_d_objs = init_system.allocate_objects(ObjectType::PageTable, pd_d_names, None);
    let vm_d_objs = init_system.allocate_objects(ObjectType::PageTable, vm_d_names, None);

    let pd_pt_names: Vec<String> = all_pd_pts
        .iter()
        .map(|(pd_idx, vaddr)| format!("PageTable: PD={} VADDR=0x{:x}", pd_names[*pd_idx], vaddr))
        .collect();
    let vm_pt_names: Vec<String> = all_vm_pts
        .iter()
        .map(|(vm_idx, vaddr)| format!("PageTable: VM={} VADDR=0x{:x}", vm_names[*vm_idx], vaddr))
        .collect();
    let pd_pt_objs = init_system.allocate_objects(ObjectType::PageTable, pd_pt_names, None);
    let vm_pt_objs = init_system.allocate_objects(ObjectType::PageTable, vm_pt_names, None);

    // Create CNodes - all CNode objects are the same size: 128 slots.
    let mut cnode_names: Vec<String> = system
        .protection_domains
        .iter()
        .map(|pd| format!("CNode: PD={}", pd.name))
        .collect();
    let vm_cnode_names: Vec<String> = virtual_machines
        .iter()
        .map(|vm| format!("CNode: VM={}", vm.name))
        .collect();
    cnode_names.extend(vm_cnode_names);

    let cnode_objs =
        init_system.allocate_objects(ObjectType::CNode, cnode_names, Some(PD_CAP_SIZE));
    let mut cnode_objs_by_pd: HashMap<&ProtectionDomain, &Object> =
        HashMap::with_capacity(system.protection_domains.len());
    for (i, pd) in system.protection_domains.iter().enumerate() {
        cnode_objs_by_pd.insert(pd, &cnode_objs[i]);
    }

    let vm_cnode_objs = &cnode_objs[system.protection_domains.len()..];

    let mut cap_slot = init_system.cap_slot;
    let kernel_objects = init_system.objects;

    // Create all the necessary interrupt handler objects. These aren't
    // created through retype though!
    let mut irq_cap_addresses: HashMap<&ProtectionDomain, Vec<u64>> = HashMap::new();
    for pd in &system.protection_domains {
        irq_cap_addresses.insert(pd, vec![]);
        for sysirq in &pd.irqs {
            let cap_address = system_cap_address_mask | cap_slot;
            system_invocations.push(Invocation::new(
                config,
                InvocationArgs::IrqControlGetTrigger {
                    irq_control: IRQ_CONTROL_CAP_ADDRESS,
                    irq: sysirq.irq,
                    trigger: sysirq.trigger,
                    dest_root: root_cnode_cap,
                    dest_index: cap_address,
                    dest_depth: config.cap_address_bits,
                },
            ));

            cap_slot += 1;
            cap_address_names.insert(cap_address, format!("IRQ Handler: irq={}", sysirq.irq));
            irq_cap_addresses.get_mut(pd).unwrap().push(cap_address);
        }
    }

    // This has to be done prior to minting!
    let num_asid_invocations = system.protection_domains.len() + virtual_machines.len();
    let mut asid_invocation = Invocation::new(
        config,
        InvocationArgs::AsidPoolAssign {
            asid_pool: INIT_ASID_POOL_CAP_ADDRESS,
            vspace: vspace_objs[0].cap_addr,
        },
    );
    asid_invocation.repeat(
        num_asid_invocations as u32,
        InvocationArgs::AsidPoolAssign {
            asid_pool: 0,
            vspace: 1,
        },
    );
    system_invocations.push(asid_invocation);

    // Create copies of all caps required via minting.

    // Mint copies of required pages, while also determing what's required
    // for later mapping
    let mut pd_page_descriptors = Vec::new();
    for (pd_idx, pd) in system.protection_domains.iter().enumerate() {
        for map_set in [&pd.maps, &pd_extra_maps[pd]] {
            for mp in map_set {
                let mr = all_mr_by_name[mp.mr.as_str()];
                let mut rights: u64 = Rights::None as u64;
                let mut attrs = match config.arch {
                    Arch::Aarch64 => ArmVmAttributes::ParityEnabled as u64,
                    Arch::Riscv64 => 0,
                };
                if mp.perms & SysMapPerms::Read as u8 != 0 {
                    rights |= Rights::Read as u64;
                }
                if mp.perms & SysMapPerms::Write as u8 != 0 {
                    rights |= Rights::Write as u64;
                }
                if mp.perms & SysMapPerms::Execute as u8 == 0 {
                    match config.arch {
                        Arch::Aarch64 => attrs |= ArmVmAttributes::ExecuteNever as u64,
                        Arch::Riscv64 => attrs |= RiscvVmAttributes::ExecuteNever as u64,
                    }
                }
                if mp.cached {
                    match config.arch {
                        Arch::Aarch64 => attrs |= ArmVmAttributes::Cacheable as u64,
                        Arch::Riscv64 => {}
                    }
                }

                assert!(!mr_pages[mr].is_empty());
                assert!(util::objects_adjacent(&mr_pages[mr]));

                let mut invocation = Invocation::new(
                    config,
                    InvocationArgs::CnodeMint {
                        cnode: system_cnode_cap,
                        dest_index: cap_slot,
                        dest_depth: system_cnode_bits,
                        src_root: root_cnode_cap,
                        src_obj: mr_pages[mr][0].cap_addr,
                        src_depth: config.cap_address_bits,
                        rights,
                        badge: 0,
                    },
                );
                invocation.repeat(
                    mr_pages[mr].len() as u32,
                    InvocationArgs::CnodeMint {
                        cnode: 0,
                        dest_index: 1,
                        dest_depth: 0,
                        src_root: 0,
                        src_obj: 1,
                        src_depth: 0,
                        rights: 0,
                        badge: 0,
                    },
                );
                system_invocations.push(invocation);

                pd_page_descriptors.push((
                    system_cap_address_mask | cap_slot,
                    pd_idx,
                    mp.vaddr,
                    rights,
                    attrs,
                    mr_pages[mr].len() as u64,
                    mr.page_bytes(),
                ));

                for idx in 0..mr_pages[mr].len() {
                    cap_address_names.insert(
                        system_cap_address_mask | (cap_slot + idx as u64),
                        format!(
                            "{} (derived)",
                            cap_address_names
                                .get(&(mr_pages[mr][0].cap_addr + idx as u64))
                                .unwrap()
                        ),
                    );
                }

                cap_slot += mr_pages[mr].len() as u64;
            }
        }
    }

    let mut vm_page_descriptors = Vec::new();
    for (vm_idx, vm) in virtual_machines.iter().enumerate() {
        for mp in &vm.maps {
            let mr = all_mr_by_name[mp.mr.as_str()];
            let mut rights: u64 = Rights::None as u64;
            let mut attrs = match config.arch {
                Arch::Aarch64 => ArmVmAttributes::ParityEnabled as u64,
                Arch::Riscv64 => 0,
            };
            if mp.perms & SysMapPerms::Read as u8 != 0 {
                rights |= Rights::Read as u64;
            }
            if mp.perms & SysMapPerms::Write as u8 != 0 {
                rights |= Rights::Write as u64;
            }
            if mp.perms & SysMapPerms::Execute as u8 == 0 {
                match config.arch {
                    Arch::Aarch64 => attrs |= ArmVmAttributes::ExecuteNever as u64,
                    Arch::Riscv64 => attrs |= RiscvVmAttributes::ExecuteNever as u64,
                }
            }
            if mp.cached {
                match config.arch {
                    Arch::Aarch64 => attrs |= ArmVmAttributes::Cacheable as u64,
                    Arch::Riscv64 => {}
                }
            }

            assert!(!mr_pages[mr].is_empty());
            assert!(util::objects_adjacent(&mr_pages[mr]));

            let mut invocation = Invocation::new(
                config,
                InvocationArgs::CnodeMint {
                    cnode: system_cnode_cap,
                    dest_index: cap_slot,
                    dest_depth: system_cnode_bits,
                    src_root: root_cnode_cap,
                    src_obj: mr_pages[mr][0].cap_addr,
                    src_depth: config.cap_address_bits,
                    rights,
                    badge: 0,
                },
            );
            invocation.repeat(
                mr_pages[mr].len() as u32,
                InvocationArgs::CnodeMint {
                    cnode: 0,
                    dest_index: 1,
                    dest_depth: 0,
                    src_root: 0,
                    src_obj: 1,
                    src_depth: 0,
                    rights: 0,
                    badge: 0,
                },
            );
            system_invocations.push(invocation);

            vm_page_descriptors.push((
                system_cap_address_mask | cap_slot,
                vm_idx,
                mp.vaddr,
                rights,
                attrs,
                mr_pages[mr].len() as u64,
                mr.page_bytes(),
            ));

            for idx in 0..mr_pages[mr].len() {
                cap_address_names.insert(
                    system_cap_address_mask | (cap_slot + idx as u64),
                    format!(
                        "{} (derived)",
                        cap_address_names
                            .get(&(mr_pages[mr][0].cap_addr + idx as u64))
                            .unwrap()
                    ),
                );
            }

            cap_slot += mr_pages[mr].len() as u64;
        }
    }

    let mut badged_irq_caps: HashMap<&ProtectionDomain, Vec<u64>> = HashMap::new();
    for (notification_obj, pd) in zip(&notification_objs, &system.protection_domains) {
        badged_irq_caps.insert(pd, vec![]);
        for sysirq in &pd.irqs {
            let badge = 1 << sysirq.id;
            let badged_cap_address = system_cap_address_mask | cap_slot;
            system_invocations.push(Invocation::new(
                config,
                InvocationArgs::CnodeMint {
                    cnode: system_cnode_cap,
                    dest_index: cap_slot,
                    dest_depth: system_cnode_bits,
                    src_root: root_cnode_cap,
                    src_obj: notification_obj.cap_addr,
                    src_depth: config.cap_address_bits,
                    rights: Rights::All as u64,
                    badge,
                },
            ));
            let badged_name = format!(
                "{} (badge=0x{:x})",
                cap_address_names[&notification_obj.cap_addr], badge
            );
            cap_address_names.insert(badged_cap_address, badged_name);
            badged_irq_caps
                .get_mut(pd)
                .unwrap()
                .push(badged_cap_address);
            cap_slot += 1;
        }
    }

    // Create a fault endpoint cap for each protection domain.
    // For root PDs, this shall be the system fault EP endpoint object.
    // For non-root PDs, this shall be the parent endpoint.
    let badged_fault_ep = system_cap_address_mask | cap_slot;
    for (i, pd) in system.protection_domains.iter().enumerate() {
        let is_root = pd.parent.is_none();
        let fault_ep_cap;
        let badge: u64;
        if is_root {
            fault_ep_cap = fault_ep_endpoint_object.cap_addr;
            badge = i as u64 + 1;
        } else {
            assert!(pd.id.is_some());
            assert!(pd.parent.is_some());
            fault_ep_cap = pd_endpoint_objs[pd.parent.unwrap()].unwrap().cap_addr;
            badge = FAULT_BADGE | pd.id.unwrap();
        }

        let invocation = Invocation::new(
            config,
            InvocationArgs::CnodeMint {
                cnode: system_cnode_cap,
                dest_index: cap_slot,
                dest_depth: system_cnode_bits,
                src_root: root_cnode_cap,
                src_obj: fault_ep_cap,
                src_depth: config.cap_address_bits,
                rights: Rights::All as u64,
                badge,
            },
        );
        system_invocations.push(invocation);
        cap_slot += 1;
    }

    // Create a fault endpoint cap for each virtual machine.
    // This will be the endpoint for the parent protection domain of the virtual machine.
    for vm in &virtual_machines {
        let mut parent_pd = None;
        for (pd_idx, pd) in system.protection_domains.iter().enumerate() {
            if let Some(virtual_machine) = &pd.virtual_machine {
                if virtual_machine == *vm {
                    parent_pd = Some(pd_idx);
                    break;
                }
            }
        }
        assert!(parent_pd.is_some());

        let fault_ep_cap = pd_endpoint_objs[parent_pd.unwrap()].unwrap().cap_addr;

        for vcpu in &vm.vcpus {
            let badge = FAULT_BADGE | vcpu.id;

            let invocation = Invocation::new(
                config,
                InvocationArgs::CnodeMint {
                    cnode: system_cnode_cap,
                    dest_index: cap_slot,
                    dest_depth: system_cnode_bits,
                    src_root: root_cnode_cap,
                    src_obj: fault_ep_cap,
                    src_depth: config.cap_address_bits,
                    rights: Rights::All as u64,
                    badge,
                },
            );
            system_invocations.push(invocation);
            cap_slot += 1;
        }
    }

    let final_cap_slot = cap_slot;

    // Minting in the address space
    for (idx, pd) in system.protection_domains.iter().enumerate() {
        let obj = if pd.needs_ep() {
            pd_endpoint_objs[idx].unwrap()
        } else {
            &notification_objs[idx]
        };
        assert!(INPUT_CAP_IDX < PD_CAP_SIZE);

        system_invocations.push(Invocation::new(
            config,
            InvocationArgs::CnodeMint {
                cnode: cnode_objs[idx].cap_addr,
                dest_index: INPUT_CAP_IDX,
                dest_depth: PD_CAP_BITS,
                src_root: root_cnode_cap,
                src_obj: obj.cap_addr,
                src_depth: config.cap_address_bits,
                rights: Rights::All as u64,
                badge: 0,
            },
        ));
    }

    // Mint access to the reply cap
    assert!(REPLY_CAP_IDX < PD_CAP_SIZE);
    let mut reply_mint_invocation = Invocation::new(
        config,
        InvocationArgs::CnodeMint {
            cnode: cnode_objs[0].cap_addr,
            dest_index: REPLY_CAP_IDX,
            dest_depth: PD_CAP_BITS,
            src_root: root_cnode_cap,
            src_obj: pd_reply_objs[0].cap_addr,
            src_depth: config.cap_address_bits,
            rights: Rights::All as u64,
            badge: 1,
        },
    );
    reply_mint_invocation.repeat(
        system.protection_domains.len() as u32,
        InvocationArgs::CnodeMint {
            cnode: 1,
            dest_index: 0,
            dest_depth: 0,
            src_root: 0,
            src_obj: 1,
            src_depth: 0,
            rights: 0,
            badge: 0,
        },
    );
    system_invocations.push(reply_mint_invocation);

    // Mint access to the VSpace cap
    assert!(VSPACE_CAP_IDX < PD_CAP_SIZE);
    let num_vspace_mint_invocations = system.protection_domains.len() + virtual_machines.len();
    let mut vspace_mint_invocation = Invocation::new(
        config,
        InvocationArgs::CnodeMint {
            cnode: cnode_objs[0].cap_addr,
            dest_index: VSPACE_CAP_IDX,
            dest_depth: PD_CAP_BITS,
            src_root: root_cnode_cap,
            src_obj: vspace_objs[0].cap_addr,
            src_depth: config.cap_address_bits,
            rights: Rights::All as u64,
            badge: 0,
        },
    );
    vspace_mint_invocation.repeat(
        num_vspace_mint_invocations as u32,
        InvocationArgs::CnodeMint {
            cnode: 1,
            dest_index: 0,
            dest_depth: 0,
            src_root: 0,
            src_obj: 1,
            src_depth: 0,
            rights: 0,
            badge: 0,
        },
    );
    system_invocations.push(vspace_mint_invocation);

    // Mint access to interrupt handlers in the PD CSpace
    for (pd_idx, pd) in system.protection_domains.iter().enumerate() {
        for (sysirq, irq_cap_address) in zip(&pd.irqs, &irq_cap_addresses[pd]) {
            let cap_idx = BASE_IRQ_CAP + sysirq.id;
            assert!(cap_idx < PD_CAP_SIZE);
            system_invocations.push(Invocation::new(
                config,
                InvocationArgs::CnodeMint {
                    cnode: cnode_objs[pd_idx].cap_addr,
                    dest_index: cap_idx,
                    dest_depth: PD_CAP_BITS,
                    src_root: root_cnode_cap,
                    src_obj: *irq_cap_address,
                    src_depth: config.cap_address_bits,
                    rights: Rights::All as u64,
                    badge: 0,
                },
            ));
        }
    }

    // Mint access to the child TCB in the CSpace of root PDs
    for (pd_idx, _) in system.protection_domains.iter().enumerate() {
        for (maybe_child_idx, maybe_child_pd) in system.protection_domains.iter().enumerate() {
            // Before doing anything, check if we are dealing with a child PD
            if let Some(parent_idx) = maybe_child_pd.parent {
                // We are dealing with a child PD, now check if the index of its parent
                // matches this iteration's PD.
                if parent_idx == pd_idx {
                    let cap_idx = BASE_PD_TCB_CAP + maybe_child_pd.id.unwrap();
                    assert!(cap_idx < PD_CAP_SIZE);
                    system_invocations.push(Invocation::new(
                        config,
                        InvocationArgs::CnodeMint {
                            cnode: cnode_objs[pd_idx].cap_addr,
                            dest_index: cap_idx,
                            dest_depth: PD_CAP_BITS,
                            src_root: root_cnode_cap,
                            src_obj: tcb_objs[maybe_child_idx].cap_addr,
                            src_depth: config.cap_address_bits,
                            rights: Rights::All as u64,
                            badge: 0,
                        },
                    ));
                }
            }
        }
    }

    // Mint access to virtual machine TCBs in the CSpace of parent PDs
    for (pd_idx, pd) in system.protection_domains.iter().enumerate() {
        if let Some(vm) = &pd.virtual_machine {
            // This PD that we are dealing with has a virtual machine, now we
            // need to find the TCB that corresponds to it.
            let vm_idx = virtual_machines.iter().position(|&x| x == vm).unwrap();

            for (vcpu_idx, vcpu) in vm.vcpus.iter().enumerate() {
                let cap_idx = BASE_VM_TCB_CAP + vcpu.id;
                assert!(cap_idx < PD_CAP_SIZE);
                system_invocations.push(Invocation::new(
                    config,
                    InvocationArgs::CnodeMint {
                        cnode: cnode_objs[pd_idx].cap_addr,
                        dest_index: cap_idx,
                        dest_depth: PD_CAP_BITS,
                        src_root: root_cnode_cap,
                        src_obj: vcpu_tcb_objs[vm_idx + vcpu_idx].cap_addr,
                        src_depth: config.cap_address_bits,
                        rights: Rights::All as u64,
                        badge: 0,
                    },
                ));
            }
        }
    }

    // Mint access to virtual machine vCPUs in the CSpace of the parent PDs
    for (pd_idx, pd) in system.protection_domains.iter().enumerate() {
        if let Some(vm) = &pd.virtual_machine {
            // This PD that we are dealing with has a virtual machine, now we
            // need to find the vCPU that corresponds to it.
            let vm_idx = virtual_machines.iter().position(|&x| x == vm).unwrap();

            for (vcpu_idx, vcpu) in vm.vcpus.iter().enumerate() {
                let cap_idx = BASE_VCPU_CAP + vcpu.id;
                assert!(cap_idx < PD_CAP_SIZE);
                system_invocations.push(Invocation::new(
                    config,
                    InvocationArgs::CnodeMint {
                        cnode: cnode_objs[pd_idx].cap_addr,
                        dest_index: cap_idx,
                        dest_depth: PD_CAP_BITS,
                        src_root: root_cnode_cap,
                        src_obj: vcpu_objs[vm_idx + vcpu_idx].cap_addr,
                        src_depth: config.cap_address_bits,
                        rights: Rights::All as u64,
                        badge: 0,
                    },
                ));
            }
        }
    }

    for cc in &system.channels {
        let pd_a = &system.protection_domains[cc.pd_a];
        let pd_b = &system.protection_domains[cc.pd_b];
        let pd_a_cnode_obj = cnode_objs_by_pd[pd_a];
        let pd_b_cnode_obj = cnode_objs_by_pd[pd_b];
        let pd_a_notification_obj = &notification_objs[cc.pd_a];
        let pd_b_notification_obj = &notification_objs[cc.pd_b];

        // Set up the notification caps
        let pd_a_cap_idx = BASE_OUTPUT_NOTIFICATION_CAP + cc.id_a;
        let pd_a_badge = 1 << cc.id_b;
        assert!(pd_a_cap_idx < PD_CAP_SIZE);
        system_invocations.push(Invocation::new(
            config,
            InvocationArgs::CnodeMint {
                cnode: pd_a_cnode_obj.cap_addr,
                dest_index: pd_a_cap_idx,
                dest_depth: PD_CAP_BITS,
                src_root: root_cnode_cap,
                src_obj: pd_b_notification_obj.cap_addr,
                src_depth: config.cap_address_bits,
                rights: Rights::All as u64, // FIXME: Check rights
                badge: pd_a_badge,
            },
        ));

        let pd_b_cap_idx = BASE_OUTPUT_NOTIFICATION_CAP + cc.id_b;
        let pd_b_badge = 1 << cc.id_a;
        assert!(pd_b_cap_idx < PD_CAP_SIZE);
        system_invocations.push(Invocation::new(
            config,
            InvocationArgs::CnodeMint {
                cnode: pd_b_cnode_obj.cap_addr,
                dest_index: pd_b_cap_idx,
                dest_depth: PD_CAP_BITS,
                src_root: root_cnode_cap,
                src_obj: pd_a_notification_obj.cap_addr,
                src_depth: config.cap_address_bits,
                rights: Rights::All as u64, // FIXME: Check rights
                badge: pd_b_badge,
            },
        ));

        // Set up the endpoint caps
        if pd_b.pp {
            let pd_a_cap_idx = BASE_OUTPUT_ENDPOINT_CAP + cc.id_a;
            let pd_a_badge = PPC_BADGE | cc.id_b;
            let pd_b_endpoint_obj = pd_endpoint_objs[cc.pd_b].unwrap();
            assert!(pd_a_cap_idx < PD_CAP_SIZE);

            system_invocations.push(Invocation::new(
                config,
                InvocationArgs::CnodeMint {
                    cnode: pd_a_cnode_obj.cap_addr,
                    dest_index: pd_a_cap_idx,
                    dest_depth: PD_CAP_BITS,
                    src_root: root_cnode_cap,
                    src_obj: pd_b_endpoint_obj.cap_addr,
                    src_depth: config.cap_address_bits,
                    rights: Rights::All as u64, // FIXME: Check rights
                    badge: pd_a_badge,
                },
            ));
        }

        if pd_a.pp {
            let pd_b_cap_idx = BASE_OUTPUT_ENDPOINT_CAP + cc.id_b;
            let pd_b_badge = PPC_BADGE | cc.id_a;
            let pd_a_endpoint_obj = pd_endpoint_objs[cc.pd_a].unwrap();
            assert!(pd_b_cap_idx < PD_CAP_SIZE);

            system_invocations.push(Invocation::new(
                config,
                InvocationArgs::CnodeMint {
                    cnode: pd_b_cnode_obj.cap_addr,
                    dest_index: pd_b_cap_idx,
                    dest_depth: PD_CAP_BITS,
                    src_root: root_cnode_cap,
                    src_obj: pd_a_endpoint_obj.cap_addr,
                    src_depth: config.cap_address_bits,
                    rights: Rights::All as u64, // FIXME: Check rights
                    badge: pd_b_badge,
                },
            ));
        }
    }

    // Mint a cap between monitor and passive PDs.
    for (pd_idx, pd) in system.protection_domains.iter().enumerate() {
        if pd.passive {
            let cnode_obj = &cnode_objs[pd_idx];
            system_invocations.push(Invocation::new(
                config,
                InvocationArgs::CnodeMint {
                    cnode: cnode_obj.cap_addr,
                    dest_index: MONITOR_EP_CAP_IDX,
                    dest_depth: PD_CAP_BITS,
                    src_root: root_cnode_cap,
                    src_obj: fault_ep_endpoint_object.cap_addr,
                    src_depth: config.cap_address_bits,
                    rights: Rights::All as u64, // FIXME: Check rights
                    // Badge needs to start at 1
                    badge: pd_idx as u64 + 1,
                },
            ));
        }
    }

    for (pd_idx, pd) in system.protection_domains.iter().enumerate() {
        if pd.smc {
            assert!(config.arm_smc.is_some() && config.arm_smc.unwrap());
            let cnode_obj = &cnode_objs[pd_idx];
            system_invocations.push(Invocation::new(
                config,
                InvocationArgs::CnodeMint {
                    cnode: cnode_obj.cap_addr,
                    dest_index: SMC_CAP_IDX,
                    dest_depth: PD_CAP_BITS,
                    src_root: root_cnode_cap,
                    src_obj: SMC_CAP_ADDRESS,
                    src_depth: config.cap_address_bits,
                    rights: Rights::All as u64, // FIXME: Check rights
                    badge: 0,
                },
            ));
        }
    }

    // All minting is complete at this point

    // Associate badges
    // FIXME: This could use repeat
    for pd in &system.protection_domains {
        for (irq_cap_address, badged_notification_cap_address) in
            zip(&irq_cap_addresses[pd], &badged_irq_caps[pd])
        {
            system_invocations.push(Invocation::new(
                config,
                InvocationArgs::IrqHandlerSetNotification {
                    irq_handler: *irq_cap_address,
                    notification: *badged_notification_cap_address,
                },
            ));
        }
    }

    // Initialise the VSpaces -- assign them all the the initial asid pool.
    let pd_vspace_invocations = [
        (all_pd_uds, pd_ud_objs),
        (all_pd_ds, pd_d_objs),
        (all_pd_pts, pd_pt_objs),
    ];
    for (descriptors, objects) in pd_vspace_invocations {
        for ((pd_idx, vaddr), obj) in zip(descriptors, objects) {
            system_invocations.push(Invocation::new(
                config,
                InvocationArgs::PageTableMap {
                    page_table: obj.cap_addr,
                    vspace: pd_vspace_objs[pd_idx].cap_addr,
                    vaddr,
                    attr: default_vm_attr(config),
                },
            ));
        }
    }

    if !config.hypervisor {
        assert!(all_vm_uds.is_empty() && vm_ud_objs.is_empty());
        assert!(all_vm_ds.is_empty() && vm_d_objs.is_empty());
        assert!(all_vm_pts.is_empty() && vm_pt_objs.is_empty());
    }

    let vm_vspace_invocations = [
        (all_vm_uds, vm_ud_objs),
        (all_vm_ds, vm_d_objs),
        (all_vm_pts, vm_pt_objs),
    ];
    for (descriptors, objects) in vm_vspace_invocations {
        for ((vm_idx, vaddr), obj) in zip(descriptors, objects) {
            system_invocations.push(Invocation::new(
                config,
                InvocationArgs::PageTableMap {
                    page_table: obj.cap_addr,
                    vspace: vm_vspace_objs[vm_idx].cap_addr,
                    vaddr,
                    attr: default_vm_attr(config),
                },
            ));
        }
    }

    // Now map all the pages
    for (page_cap_address, pd_idx, vaddr, rights, attr, count, vaddr_incr) in pd_page_descriptors {
        let mut invocation = Invocation::new(
            config,
            InvocationArgs::PageMap {
                page: page_cap_address,
                vspace: pd_vspace_objs[pd_idx].cap_addr,
                vaddr,
                rights,
                attr,
            },
        );
        invocation.repeat(
            count as u32,
            InvocationArgs::PageMap {
                page: 1,
                vspace: 0,
                vaddr: vaddr_incr,
                rights: 0,
                attr: 0,
            },
        );
        system_invocations.push(invocation);
    }
    for (page_cap_address, vm_idx, vaddr, rights, attr, count, vaddr_incr) in vm_page_descriptors {
        let mut invocation = Invocation::new(
            config,
            InvocationArgs::PageMap {
                page: page_cap_address,
                vspace: vm_vspace_objs[vm_idx].cap_addr,
                vaddr,
                rights,
                attr,
            },
        );
        invocation.repeat(
            count as u32,
            InvocationArgs::PageMap {
                page: 1,
                vspace: 0,
                vaddr: vaddr_incr,
                rights: 0,
                attr: 0,
            },
        );
        system_invocations.push(invocation);
    }

    // And, finally, map all the IPC buffers
    let ipc_buffer_attr = match config.arch {
        Arch::Aarch64 => ArmVmAttributes::default() | ArmVmAttributes::ExecuteNever as u64,
        Arch::Riscv64 => RiscvVmAttributes::default() | RiscvVmAttributes::ExecuteNever as u64,
    };
    for pd_idx in 0..system.protection_domains.len() {
        let (vaddr, _) = pd_elf_files[pd_idx]
            .find_symbol(SYMBOL_IPC_BUFFER)
            .unwrap_or_else(|_| panic!("Could not find {}", SYMBOL_IPC_BUFFER));
        system_invocations.push(Invocation::new(
            config,
            InvocationArgs::PageMap {
                page: ipc_buffer_objs[pd_idx].cap_addr,
                vspace: pd_vspace_objs[pd_idx].cap_addr,
                vaddr,
                rights: Rights::Read as u64 | Rights::Write as u64,
                attr: ipc_buffer_attr,
            },
        ));
    }

    // Initialise the TCBs

    // Set the scheduling parameters
    for (pd_idx, pd) in system.protection_domains.iter().enumerate() {
        system_invocations.push(Invocation::new(
            config,
            InvocationArgs::SchedControlConfigureFlags {
                sched_control: kernel_boot_info.sched_control_cap,
                sched_context: pd_sched_context_objs[pd_idx].cap_addr,
                budget: pd.budget,
                period: pd.period,
                extra_refills: 0,
                badge: 0x100 + pd_idx as u64,
                flags: 0,
            },
        ));
    }
    for (vm_idx, vm) in virtual_machines.iter().enumerate() {
        for vcpu_idx in 0..vm.vcpus.len() {
            let idx = vm_idx + vcpu_idx;
            system_invocations.push(Invocation::new(
                config,
                InvocationArgs::SchedControlConfigureFlags {
                    sched_control: kernel_boot_info.sched_control_cap,
                    sched_context: vm_sched_context_objs[idx].cap_addr,
                    budget: vm.budget,
                    period: vm.period,
                    extra_refills: 0,
                    badge: 0x100 + idx as u64,
                    flags: 0,
                },
            ));
        }
    }

    for (pd_idx, pd) in system.protection_domains.iter().enumerate() {
        system_invocations.push(Invocation::new(
            config,
            InvocationArgs::TcbSetSchedParams {
                tcb: pd_tcb_objs[pd_idx].cap_addr,
                authority: INIT_TCB_CAP_ADDRESS,
                mcp: pd.priority as u64,
                priority: pd.priority as u64,
                sched_context: pd_sched_context_objs[pd_idx].cap_addr,
                // This gets over-written by the call to TCB_SetSpace
                fault_ep: fault_ep_endpoint_object.cap_addr,
            },
        ));
    }
    for (vm_idx, vm) in virtual_machines.iter().enumerate() {
        for vcpu_idx in 0..vm.vcpus.len() {
            system_invocations.push(Invocation::new(
                config,
                InvocationArgs::TcbSetSchedParams {
                    tcb: vcpu_tcb_objs[vm_idx + vcpu_idx].cap_addr,
                    authority: INIT_TCB_CAP_ADDRESS,
                    mcp: vm.priority as u64,
                    priority: vm.priority as u64,
                    sched_context: vm_sched_context_objs[vm_idx + vcpu_idx].cap_addr,
                    // This gets over-written by the call to TCB_SetSpace
                    fault_ep: fault_ep_endpoint_object.cap_addr,
                },
            ));
        }
    }

    // In the benchmark configuration, we allow PDs to access their own TCB.
    // This is necessary for accessing kernel's benchmark API.
    if config.benchmark {
        let mut tcb_cap_copy_invocation = Invocation::new(
            config,
            InvocationArgs::CnodeCopy {
                cnode: cnode_objs[0].cap_addr,
                dest_index: TCB_CAP_IDX,
                dest_depth: PD_CAP_BITS,
                src_root: root_cnode_cap,
                src_obj: pd_tcb_objs[0].cap_addr,
                src_depth: config.cap_address_bits,
                rights: Rights::All as u64,
            },
        );
        tcb_cap_copy_invocation.repeat(
            system.protection_domains.len() as u32,
            InvocationArgs::CnodeCopy {
                cnode: 1,
                dest_index: 0,
                dest_depth: 0,
                src_root: 0,
                src_obj: 1,
                src_depth: 0,
                rights: 0,
            },
        );
        system_invocations.push(tcb_cap_copy_invocation);
    }

    // Set VSpace and CSpace
    let mut pd_set_space_invocation = Invocation::new(
        config,
        InvocationArgs::TcbSetSpace {
            tcb: tcb_objs[0].cap_addr,
            fault_ep: badged_fault_ep,
            cspace_root: cnode_objs[0].cap_addr,
            cspace_root_data: config.cap_address_bits - PD_CAP_BITS,
            vspace_root: vspace_objs[0].cap_addr,
            vspace_root_data: 0,
        },
    );
    pd_set_space_invocation.repeat(
        system.protection_domains.len() as u32,
        InvocationArgs::TcbSetSpace {
            tcb: 1,
            fault_ep: 1,
            cspace_root: 1,
            cspace_root_data: 0,
            vspace_root: 1,
            vspace_root_data: 0,
        },
    );
    system_invocations.push(pd_set_space_invocation);

    for (vm_idx, vm) in virtual_machines.iter().enumerate() {
        let fault_ep_offset = system.protection_domains.len() + vm_idx;
        let mut vcpu_set_space_invocation = Invocation::new(
            config,
            InvocationArgs::TcbSetSpace {
                tcb: vcpu_tcb_objs[vm_idx].cap_addr,
                fault_ep: badged_fault_ep + fault_ep_offset as u64,
                cspace_root: vm_cnode_objs[vm_idx].cap_addr,
                cspace_root_data: config.cap_address_bits - PD_CAP_BITS,
                vspace_root: vm_vspace_objs[vm_idx].cap_addr,
                vspace_root_data: 0,
            },
        );
        vcpu_set_space_invocation.repeat(
            vm.vcpus.len() as u32,
            InvocationArgs::TcbSetSpace {
                tcb: 1,
                fault_ep: 1,
                cspace_root: 0,
                cspace_root_data: 0,
                vspace_root: 0,
                vspace_root_data: 0,
            },
        );
        system_invocations.push(vcpu_set_space_invocation);
    }

    // Set IPC buffer
    for pd_idx in 0..system.protection_domains.len() {
        let (ipc_buffer_vaddr, _) = pd_elf_files[pd_idx]
            .find_symbol(SYMBOL_IPC_BUFFER)
            .unwrap_or_else(|_| panic!("Could not find {}", SYMBOL_IPC_BUFFER));
        system_invocations.push(Invocation::new(
            config,
            InvocationArgs::TcbSetIpcBuffer {
                tcb: tcb_objs[pd_idx].cap_addr,
                buffer: ipc_buffer_vaddr,
                buffer_frame: ipc_buffer_objs[pd_idx].cap_addr,
            },
        ));
    }

    // Set TCB registers (we only set the entry point)
    for pd_idx in 0..system.protection_domains.len() {
        let regs = match config.arch {
            Arch::Aarch64 => Aarch64Regs {
                pc: pd_elf_files[pd_idx].entry,
                sp: config.pd_stack_top(),
                ..Default::default()
            }
            .field_names(),
            Arch::Riscv64 => Riscv64Regs {
                pc: pd_elf_files[pd_idx].entry,
                sp: config.pd_stack_top(),
                ..Default::default()
            }
            .field_names(),
        };

        system_invocations.push(Invocation::new(
            config,
            InvocationArgs::TcbWriteRegisters {
                tcb: tcb_objs[pd_idx].cap_addr,
                resume: false,
                // There are no arch-dependent flags to set
                arch_flags: 0,
                // FIXME: we could optimise this since we are only setting the program counter
                count: regs.len() as u64,
                regs,
            },
        ));
    }

    // Bind the notification object
    let mut bind_ntfn_invocation = Invocation::new(
        config,
        InvocationArgs::TcbBindNotification {
            tcb: tcb_objs[0].cap_addr,
            notification: notification_objs[0].cap_addr,
        },
    );
    bind_ntfn_invocation.repeat(
        system.protection_domains.len() as u32,
        InvocationArgs::TcbBindNotification {
            tcb: 1,
            notification: 1,
        },
    );
    system_invocations.push(bind_ntfn_invocation);

    // Bind virtual machine TCBs to vCPUs
    if !virtual_machines.is_empty() {
        match config.arch {
            Arch::Aarch64 => {}
            _ => panic!("Support for virtual machines is only for AArch64"),
        }
        let mut vcpu_bind_invocation = Invocation::new(
            config,
            InvocationArgs::ArmVcpuSetTcb {
                vcpu: vcpu_objs[0].cap_addr,
                tcb: vcpu_tcb_objs[0].cap_addr,
            },
        );
        let num_vcpus = virtual_machines
            .iter()
            .fold(0, |acc, vm| acc + vm.vcpus.len());
        vcpu_bind_invocation.repeat(
            num_vcpus as u32,
            InvocationArgs::ArmVcpuSetTcb { vcpu: 1, tcb: 1 },
        );
        system_invocations.push(vcpu_bind_invocation);
    }

    // Resume (start) all the threads that belong to PDs (VMs are not started upon system init)
    let mut resume_invocation = Invocation::new(
        config,
        InvocationArgs::TcbResume {
            tcb: tcb_objs[0].cap_addr,
        },
    );
    resume_invocation.repeat(
        system.protection_domains.len() as u32,
        InvocationArgs::TcbResume { tcb: 1 },
    );
    system_invocations.push(resume_invocation);

    // All of the objects are created at this point; we don't need both
    // the allocators from here.

    // And now we are finally done. We have all the invocations

    let mut system_invocation_data: Vec<u8> = Vec::new();
    for system_invocation in &system_invocations {
        system_invocation.add_raw_invocation(config, &mut system_invocation_data);
    }

    let mut pd_setvar_values: Vec<Vec<u64>> = vec![vec![]; system.protection_domains.len()];
    for (i, pd) in system.protection_domains.iter().enumerate() {
        for setvar in &pd.setvars {
            assert!(setvar.region_paddr.is_some() || setvar.vaddr.is_some());
            assert!(!(setvar.region_paddr.is_some() && setvar.vaddr.is_some()));

            let value;
            if let Some(region_paddr) = &setvar.region_paddr {
                let mr = system
                    .memory_regions
                    .iter()
                    .find(|mr| mr.name == *region_paddr)
                    .unwrap_or_else(|| panic!("Cannot find region: {}", region_paddr));
                value = mr_pages[mr][0].phys_addr;
            } else if let Some(vaddr) = setvar.vaddr {
                value = vaddr;
            } else {
                panic!("Internal error: expected setvar to either have region paddr or vaddr");
            }

            pd_setvar_values[i].push(value);
        }
    }

    Ok(BuiltSystem {
        number_of_system_caps: final_cap_slot,
        invocation_data_size: system_invocation_data.len() as u64,
        invocation_data: system_invocation_data,
        bootstrap_invocations,
        system_invocations,
        kernel_boot_info,
        reserved_region,
        fault_ep_cap_address: fault_ep_endpoint_object.cap_addr,
        reply_cap_address: reply_obj.cap_addr,
        cap_lookup: cap_address_names,
        tcb_caps: tcb_caps[..system.protection_domains.len()].to_vec(),
        sched_caps: sched_context_caps,
        ntfn_caps: notification_caps,
        pd_elf_regions,
        pd_setvar_values,
        kernel_objects,
        initial_task_phys_region,
        initial_task_virt_region,
    })
}

fn write_report<W: std::io::Write>(
    buf: &mut BufWriter<W>,
    config: &Config,
    built_system: &BuiltSystem,
    bootstrap_invocation_data: &[u8],
) -> std::io::Result<()> {
    writeln!(buf, "# Kernel Boot Info\n")?;

    writeln!(
        buf,
        "    # of fixed caps     : {:>8}",
        comma_sep_u64(built_system.kernel_boot_info.fixed_cap_count)
    )?;
    writeln!(
        buf,
        "    # of page table caps: {:>8}",
        comma_sep_u64(built_system.kernel_boot_info.paging_cap_count)
    )?;
    writeln!(
        buf,
        "    # of page caps      : {:>8}",
        comma_sep_u64(built_system.kernel_boot_info.page_cap_count)
    )?;
    writeln!(
        buf,
        "    # of untyped objects: {:>8}",
        comma_sep_usize(built_system.kernel_boot_info.untyped_objects.len())
    )?;
    writeln!(buf, "\n# Loader Regions\n")?;
    for regions in &built_system.pd_elf_regions {
        for region in regions {
            writeln!(buf, "       {}", region)?;
        }
    }
    writeln!(buf, "\n# Monitor (Initial Task) Info\n")?;
    writeln!(
        buf,
        "     virtual memory : {}",
        built_system.initial_task_virt_region
    )?;
    writeln!(
        buf,
        "     physical memory: {}",
        built_system.initial_task_phys_region
    )?;
    writeln!(buf, "\n# Allocated Kernel Objects Summary\n")?;
    writeln!(
        buf,
        "     # of allocated objects: {}",
        comma_sep_usize(built_system.kernel_objects.len())
    )?;
    writeln!(buf, "\n# Bootstrap Kernel Invocations Summary\n")?;
    writeln!(
        buf,
        "     # of invocations   : {:>10}",
        comma_sep_usize(built_system.bootstrap_invocations.len())
    )?;
    writeln!(
        buf,
        "     size of invocations: {:>10}",
        comma_sep_usize(bootstrap_invocation_data.len())
    )?;
    writeln!(buf, "\n# System Kernel Invocations Summary\n")?;
    writeln!(
        buf,
        "     # of invocations   : {:>10}",
        comma_sep_usize(built_system.system_invocations.len())
    )?;
    writeln!(
        buf,
        "     size of invocations: {:>10}",
        comma_sep_usize(built_system.invocation_data.len())
    )?;
    writeln!(buf, "\n# Allocated Kernel Objects Detail\n")?;
    for ko in &built_system.kernel_objects {
        // FIXME: would be good to print both the number for the object type and the string
        let name = built_system.cap_lookup.get(&ko.cap_addr).unwrap();
        writeln!(
            buf,
            "    {:<50} {} cap_addr={:x} phys_addr={:x}",
            name,
            ko.object_type.value(config),
            ko.cap_addr,
            ko.phys_addr
        )?;
    }
    writeln!(buf, "\n# Bootstrap Kernel Invocations Detail\n")?;
    for (i, invocation) in built_system.bootstrap_invocations.iter().enumerate() {
        write!(buf, "    0x{:04x} ", i)?;
        invocation.report_fmt(buf, config, &built_system.cap_lookup);
    }
    writeln!(buf, "\n# System Kernel Invocations Detail\n")?;
    for (i, invocation) in built_system.system_invocations.iter().enumerate() {
        write!(buf, "    0x{:04x} ", i)?;
        invocation.report_fmt(buf, config, &built_system.cap_lookup);
    }

    Ok(())
}

fn print_usage(available_boards: &[String]) {
    println!("usage: microkit [-h] [-o OUTPUT] [-r REPORT] --board {{{}}} --config CONFIG [--search-path [SEARCH_PATH ...]] system", available_boards.join(","))
}

fn print_help(available_boards: &[String]) {
    print_usage(available_boards);
    println!("\npositional arguments:");
    println!("  system");
    println!("\noptions:");
    println!("  -h, --help, show this help message and exit");
    println!("  -o, --output OUTPUT");
    println!("  -r, --report REPORT");
    println!("  --board {{{}}}", available_boards.join(","));
    println!("  --config CONFIG");
    println!("  --search-path [SEARCH_PATH ...]");
}

struct Args<'a> {
    system: &'a str,
    board: &'a str,
    config: &'a str,
    report: &'a str,
    output: &'a str,
    search_paths: Vec<&'a String>,
}

impl<'a> Args<'a> {
    pub fn parse(args: &'a [String], available_boards: &[String]) -> Args<'a> {
        // Default arguments
        let mut output = "loader.img";
        let mut report = "report.txt";
        let mut search_paths = Vec::new();
        // Arguments expected to be provided by the user
        let mut system = None;
        let mut board = None;
        let mut config = None;

        if args.len() <= 1 {
            print_usage(available_boards);
            std::process::exit(1);
        }

        let mut i = 1;
        let mut unknown = vec![];
        let mut in_search_path = false;
        while i < args.len() {
            match args[i].as_str() {
                "-h" | "--help" => {
                    print_help(available_boards);
                    std::process::exit(0);
                }
                "-o" | "--output" => {
                    in_search_path = false;
                    if i < args.len() - 1 {
                        output = &args[i + 1];
                        i += 1;
                    } else {
                        eprintln!("microkit: error: argument -o/--output: expected one argument");
                        std::process::exit(1);
                    }
                }
                "-r" | "--report" => {
                    in_search_path = false;
                    if i < args.len() - 1 {
                        report = &args[i + 1];
                        i += 1;
                    } else {
                        eprintln!("microkit: error: argument -r/--report: expected one argument");
                        std::process::exit(1);
                    }
                }
                "--board" => {
                    in_search_path = false;
                    if i < args.len() - 1 {
                        board = Some(&args[i + 1]);
                        i += 1;
                    } else {
                        eprintln!("microkit: error: argument --board: expected one argument");
                        std::process::exit(1);
                    }
                }
                "--config" => {
                    in_search_path = false;
                    if i < args.len() - 1 {
                        config = Some(&args[i + 1]);
                        i += 1;
                    } else {
                        eprintln!("microkit: error: argument --config: expected one argument");
                        std::process::exit(1);
                    }
                }
                "--search-path" => {
                    in_search_path = true;
                }
                _ => {
                    if in_search_path {
                        search_paths.push(&args[i]);
                    } else if system.is_none() {
                        system = Some(&args[i]);
                    } else {
                        // This call to clone is okay since having unknown
                        // arguments is rare.
                        unknown.push(args[i].clone());
                    }
                }
            }

            i += 1;
        }

        if !unknown.is_empty() {
            print_usage(available_boards);
            eprintln!(
                "microkit: error: unrecognised arguments: {}",
                unknown.join(" ")
            );
            std::process::exit(1);
        }

        let mut missing_args = Vec::new();
        if board.is_none() {
            missing_args.push("--board");
        }
        if config.is_none() {
            missing_args.push("--config");
        }

        if !missing_args.is_empty() {
            print_usage(available_boards);
            eprintln!(
                "microkit: error: the following arguments are required: {}",
                missing_args.join(", ")
            );
            std::process::exit(1);
        }

        Args {
            system: system.unwrap(),
            board: board.unwrap(),
            config: config.unwrap(),
            report,
            output,
            search_paths,
        }
    }
}

fn main() -> Result<(), String> {
    let exe_path = std::env::current_exe().unwrap();
    let sdk_env = std::env::var("MICROKIT_SDK");
    let sdk_dir = match sdk_env {
        Ok(ref value) => Path::new(value),
        Err(err) => match err {
            // If there is no MICROKIT_SDK explicitly set, use the one that the binary is in.
            std::env::VarError::NotPresent => exe_path.parent().unwrap().parent().unwrap(),
            _ => {
                return Err(format!(
                    "Could not read MICROKIT_SDK environment variable: {}",
                    err
                ))
            }
        },
    };

    if !sdk_dir.exists() {
        eprintln!(
            "Error: SDK directory '{}' does not exist.",
            sdk_dir.display()
        );
        std::process::exit(1);
    }

    let boards_path = sdk_dir.join("board");
    if !boards_path.exists() || !boards_path.is_dir() {
        eprintln!(
            "Error: SDK directory '{}' does not have a 'board' sub-directory.",
            sdk_dir.display()
        );
        std::process::exit(1);
    }

    let mut available_boards = Vec::new();
    for p in fs::read_dir(&boards_path).unwrap() {
        let path_buf = p.unwrap().path();
        let path = path_buf.as_path();
        if path.is_dir() {
            available_boards.push(path.file_name().unwrap().to_str().unwrap().to_string());
        }
    }

    let env_args: Vec<_> = std::env::args().collect();
    let args = Args::parse(&env_args, &available_boards);

    let board_path = boards_path.join(args.board);
    if !board_path.exists() {
        eprintln!(
            "Error: board path '{}' does not exist.",
            board_path.display()
        );
        std::process::exit(1);
    }

    let mut available_configs = Vec::new();
    for p in fs::read_dir(board_path).unwrap() {
        let path_buf = p.unwrap().path();
        let path = path_buf.as_path();

        if path.file_name().unwrap() == "example" {
            continue;
        }

        if path.is_dir() {
            available_configs.push(path.file_name().unwrap().to_str().unwrap().to_string());
        }
    }

    if !available_configs.contains(&args.config.to_string()) {
        eprintln!(
            "microkit: error: argument --config: invalid choice: '{}' (choose from: {})",
            args.config,
            available_configs.join(", ")
        )
    }

    let elf_path = sdk_dir
        .join("board")
        .join(args.board)
        .join(args.config)
        .join("elf");
    let loader_elf_path = elf_path.join("loader.elf");
    let kernel_elf_path = elf_path.join("sel4.elf");
    let monitor_elf_path = elf_path.join("monitor.elf");

    let kernel_config_path = sdk_dir
        .join("board")
        .join(args.board)
        .join(args.config)
        .join("include/kernel/gen_config.json");

    if !elf_path.exists() {
        eprintln!(
            "Error: board ELF directory '{}' does not exist",
            elf_path.display()
        );
        std::process::exit(1);
    }
    if !loader_elf_path.exists() {
        eprintln!(
            "Error: loader ELF '{}' does not exist",
            loader_elf_path.display()
        );
        std::process::exit(1);
    }
    if !kernel_elf_path.exists() {
        eprintln!(
            "Error: kernel ELF '{}' does not exist",
            kernel_elf_path.display()
        );
        std::process::exit(1);
    }
    if !monitor_elf_path.exists() {
        eprintln!(
            "Error: monitor ELF '{}' does not exist",
            monitor_elf_path.display()
        );
        std::process::exit(1);
    }
    if !kernel_config_path.exists() {
        eprintln!(
            "Error: kernel configuration file '{}' does not exist",
            kernel_config_path.display()
        );
        std::process::exit(1);
    }

    let system_path = Path::new(args.system);
    if !system_path.exists() {
        eprintln!(
            "Error: system description file '{}' does not exist",
            system_path.display()
        );
        std::process::exit(1);
    }

    let xml: String = fs::read_to_string(args.system).unwrap();

    let kernel_config_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(kernel_config_path).unwrap()).unwrap();

    let arch = match json_str(&kernel_config_json, "SEL4_ARCH")? {
        "aarch64" => Arch::Aarch64,
        "riscv64" => Arch::Riscv64,
        _ => panic!("Unsupported kernel config architecture"),
    };

    let hypervisor = match arch {
        Arch::Aarch64 => json_str_as_bool(&kernel_config_json, "ARM_HYPERVISOR_SUPPORT")?,
        // Hypervisor mode is not available on RISC-V
        Arch::Riscv64 => false,
    };

    let arm_pa_size_bits = match arch {
        Arch::Aarch64 => {
            if json_str_as_bool(&kernel_config_json, "ARM_PA_SIZE_BITS_40")? {
                Some(40)
            } else if json_str_as_bool(&kernel_config_json, "ARM_PA_SIZE_BITS_44")? {
                Some(44)
            } else {
                panic!("Expected ARM platform to have 40 or 44 physical address bits")
            }
        }
        Arch::Riscv64 => None,
    };

    let arm_smc = match arch {
        Arch::Aarch64 => Some(json_str_as_bool(&kernel_config_json, "ALLOW_SMC_CALLS")?),
        _ => None,
    };

    let kernel_frame_size = match arch {
        Arch::Aarch64 => 1 << 12,
        Arch::Riscv64 => 1 << 21,
    };

    let kernel_config = Config {
        arch,
        word_size: json_str_as_u64(&kernel_config_json, "WORD_SIZE")?,
        minimum_page_size: 4096,
        paddr_user_device_top: json_str_as_u64(&kernel_config_json, "PADDR_USER_DEVICE_TOP")?,
        kernel_frame_size,
        init_cnode_bits: json_str_as_u64(&kernel_config_json, "ROOT_CNODE_SIZE_BITS")?,
        cap_address_bits: 64,
        fan_out_limit: json_str_as_u64(&kernel_config_json, "RETYPE_FAN_OUT_LIMIT")?,
        hypervisor,
        benchmark: args.config == "benchmark",
        fpu: json_str_as_bool(&kernel_config_json, "HAVE_FPU")?,
        arm_pa_size_bits,
        arm_smc,
        riscv_pt_levels: Some(RiscvVirtualMemory::Sv39),
    };

    if let Arch::Aarch64 = kernel_config.arch {
        assert!(
            kernel_config.hypervisor,
            "Microkit tool expects a kernel with hypervisor mode enabled on AArch64."
        );
        assert!(
            kernel_config.arm_pa_size_bits.unwrap() == 40,
            "Microkit tool has assumptions about the ARM physical address size bits"
        );
    }

    assert!(
        kernel_config.word_size == 64,
        "Microkit tool has various assumptions about the word size being 64-bits."
    );

    let system = match parse(args.system, &xml, &kernel_config) {
        Ok(system) => system,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    let monitor_config = MonitorConfig {
        untyped_info_symbol_name: "untyped_info",
        bootstrap_invocation_count_symbol_name: "bootstrap_invocation_count",
        bootstrap_invocation_data_symbol_name: "bootstrap_invocation_data",
        system_invocation_count_symbol_name: "system_invocation_count",
    };

    let kernel_elf = ElfFile::from_path(&kernel_elf_path)?;
    let mut monitor_elf = ElfFile::from_path(&monitor_elf_path)?;

    if monitor_elf.segments.iter().filter(|s| s.loadable).count() > 1 {
        eprintln!(
            "Monitor ({}) has {} segments, it must only have one",
            monitor_elf_path.display(),
            monitor_elf.segments.len()
        );
        std::process::exit(1);
    }

    let mut search_paths = vec![std::env::current_dir().unwrap()];
    for path in args.search_paths {
        search_paths.push(PathBuf::from(path));
    }

    // Get the elf files for each pd:
    let mut pd_elf_files = Vec::with_capacity(system.protection_domains.len());
    for pd in &system.protection_domains {
        match get_full_path(&pd.program_image, &search_paths) {
            Some(path) => {
                let elf = ElfFile::from_path(&path).unwrap();
                pd_elf_files.push(elf);
            }
            None => {
                return Err(format!(
                    "unable to find program image: '{}'",
                    pd.program_image.display()
                ))
            }
        }
    }

    let mut invocation_table_size = kernel_config.minimum_page_size;
    let mut system_cnode_size = 2;

    let mut built_system;
    loop {
        built_system = build_system(
            &kernel_config,
            &pd_elf_files,
            &kernel_elf,
            &monitor_elf,
            &system,
            invocation_table_size,
            system_cnode_size,
        )?;
        println!("BUILT: system_cnode_size={} built_system.number_of_system_caps={} invocation_table_size={} built_system.invocation_data_size={}",
                 system_cnode_size, built_system.number_of_system_caps, invocation_table_size, built_system.invocation_data_size);

        if built_system.number_of_system_caps <= system_cnode_size
            && built_system.invocation_data_size <= invocation_table_size
        {
            break;
        }

        // Recalculate the sizes for the next iteration
        let new_invocation_table_size = util::round_up(
            built_system.invocation_data_size,
            kernel_config.minimum_page_size,
        );
        let new_system_cnode_size = 2_u64.pow(
            built_system
                .number_of_system_caps
                .next_power_of_two()
                .ilog2(),
        );

        invocation_table_size = max(invocation_table_size, new_invocation_table_size);
        system_cnode_size = max(system_cnode_size, new_system_cnode_size);
    }

    // At this point we just need to patch the files (in memory) and write out the final image.

    // A: The monitor

    // A.1: As part of emulated boot we determined exactly how the kernel would
    // create untyped objects. Throught testing we know that this matches, but
    // we could have a bug, or the kernel could change. It that happens we are
    // in a bad spot! Things will break. So we write out this information so that
    // the monitor can double check this at run time.
    let (_, untyped_info_size) = monitor_elf
        .find_symbol(monitor_config.untyped_info_symbol_name)
        .unwrap_or_else(|_| {
            panic!(
                "Could not find '{}' symbol",
                monitor_config.untyped_info_symbol_name
            )
        });
    let max_untyped_objects = monitor_config.max_untyped_objects(untyped_info_size);
    if built_system.kernel_boot_info.untyped_objects.len() as u64 > max_untyped_objects {
        eprintln!(
            "Too many untyped objects: monitor ({}) supports {} regions. System has {} objects.",
            monitor_elf_path.display(),
            max_untyped_objects,
            built_system.kernel_boot_info.untyped_objects.len()
        );
        std::process::exit(1);
    }

    let untyped_info_header = MonitorUntypedInfoHeader64 {
        cap_start: built_system.kernel_boot_info.untyped_objects[0].cap,
        cap_end: built_system
            .kernel_boot_info
            .untyped_objects
            .last()
            .unwrap()
            .cap
            + 1,
    };
    let untyped_info_object_data: Vec<MonitorRegion64> = built_system
        .kernel_boot_info
        .untyped_objects
        .iter()
        .map(|ut| MonitorRegion64 {
            paddr: ut.base(),
            size_bits: ut.size_bits(),
            is_device: ut.is_device as u64,
        })
        .collect();
    let mut untyped_info_data: Vec<u8> =
        Vec::from(unsafe { struct_to_bytes(&untyped_info_header) });
    for o in &untyped_info_object_data {
        untyped_info_data.extend(unsafe { struct_to_bytes(o) });
    }
    monitor_elf.write_symbol(monitor_config.untyped_info_symbol_name, &untyped_info_data)?;

    let mut bootstrap_invocation_data: Vec<u8> = Vec::new();
    for invocation in &built_system.bootstrap_invocations {
        invocation.add_raw_invocation(&kernel_config, &mut bootstrap_invocation_data);
    }

    let (_, bootstrap_invocation_data_size) =
        monitor_elf.find_symbol(monitor_config.bootstrap_invocation_data_symbol_name)?;
    if bootstrap_invocation_data.len() as u64 > bootstrap_invocation_data_size {
        eprintln!(
            "bootstrap invocation array size   : {}",
            bootstrap_invocation_data_size
        );
        eprintln!(
            "bootstrap invocation required size: {}",
            bootstrap_invocation_data.len()
        );
        let mut stderr = BufWriter::new(std::io::stderr());
        for bootstrap_invocation in &built_system.bootstrap_invocations {
            bootstrap_invocation.report_fmt(&mut stderr, &kernel_config, &built_system.cap_lookup);
        }
        stderr.flush().unwrap();

        eprintln!("Internal error: bootstrap invocations too large");
    }

    monitor_elf.write_symbol(
        monitor_config.bootstrap_invocation_count_symbol_name,
        &built_system.bootstrap_invocations.len().to_le_bytes(),
    )?;
    monitor_elf.write_symbol(
        monitor_config.system_invocation_count_symbol_name,
        &built_system.system_invocations.len().to_le_bytes(),
    )?;
    monitor_elf.write_symbol(
        monitor_config.bootstrap_invocation_data_symbol_name,
        &bootstrap_invocation_data,
    )?;

    let mut tcb_cap_bytes = vec![0; (1 + built_system.tcb_caps.len()) * 8];
    for (i, cap) in built_system.tcb_caps.iter().enumerate() {
        let start = (i + 1) * 8;
        let end = start + 8;
        tcb_cap_bytes[start..end].copy_from_slice(&cap.to_le_bytes());
    }
    let mut sched_cap_bytes = vec![0; (1 + built_system.sched_caps.len()) * 8];
    for (i, cap) in built_system.sched_caps.iter().enumerate() {
        let start = (i + 1) * 8;
        let end = start + 8;
        sched_cap_bytes[start..end].copy_from_slice(&cap.to_le_bytes());
    }
    let mut ntfn_cap_bytes = vec![0; (1 + built_system.ntfn_caps.len()) * 8];
    for (i, cap) in built_system.ntfn_caps.iter().enumerate() {
        let start = (i + 1) * 8;
        let end = start + 8;
        ntfn_cap_bytes[start..end].copy_from_slice(&cap.to_le_bytes());
    }

    monitor_elf.write_symbol("fault_ep", &built_system.fault_ep_cap_address.to_le_bytes())?;
    monitor_elf.write_symbol("reply", &built_system.reply_cap_address.to_le_bytes())?;
    monitor_elf.write_symbol("tcbs", &tcb_cap_bytes)?;
    monitor_elf.write_symbol("scheduling_contexts", &sched_cap_bytes)?;
    monitor_elf.write_symbol("notification_caps", &ntfn_cap_bytes)?;
    // We do MAX_PDS + 1 due to the index that the monitor uses (the badge) starting at 1.
    let mut pd_names_bytes = vec![0; (MAX_PDS + 1) * PD_MAX_NAME_LENGTH];
    for (i, pd) in system.protection_domains.iter().enumerate() {
        // The monitor will index into the array of PD names based on the badge, which
        // starts at 1 and hence we cannot use the 0th entry in the array.
        let name = pd.name.as_bytes();
        let start = (i + 1) * PD_MAX_NAME_LENGTH;
        // Here instead of giving an error we simply take the minimum of the PD's name
        // and how large of a name we can encode
        let name_length = min(name.len(), PD_MAX_NAME_LENGTH);
        let end = start + name_length;
        pd_names_bytes[start..end].copy_from_slice(&name[..name_length]);
        // These bytes will be interpreted as a C string, so we must include
        // a null-terminator.
        pd_names_bytes[start + PD_MAX_NAME_LENGTH - 1] = 0;
    }
    monitor_elf.write_symbol("pd_names", &pd_names_bytes)?;

    // Write out all the symbols for each PD
    pd_write_symbols(
        &system.protection_domains,
        &mut pd_elf_files,
        &built_system.pd_setvar_values,
    )?;

    // Generate the report
    let report = match std::fs::File::create(args.report) {
        Ok(file) => file,
        Err(e) => {
            return Err(format!(
                "Could not create report file '{}': {}",
                args.report, e
            ))
        }
    };

    let mut report_buf = BufWriter::new(report);
    match write_report(
        &mut report_buf,
        &kernel_config,
        &built_system,
        &bootstrap_invocation_data,
    ) {
        Ok(()) => report_buf.flush().unwrap(),
        Err(err) => {
            return Err(format!(
                "Could not write out report file '{}': {}",
                args.report, err
            ))
        }
    }
    report_buf.flush().unwrap();

    let mut loader_regions: Vec<(u64, &[u8])> = vec![(
        built_system.reserved_region.base,
        &built_system.invocation_data,
    )];
    for (i, regions) in built_system.pd_elf_regions.iter().enumerate() {
        for r in regions {
            loader_regions.push((r.addr, r.data(&pd_elf_files[i])));
        }
    }

    let loader = Loader::new(
        &kernel_config,
        Path::new(&loader_elf_path),
        &kernel_elf,
        &monitor_elf,
        Some(built_system.initial_task_phys_region.base),
        built_system.reserved_region,
        loader_regions,
    );
    loader.write_image(Path::new(args.output));

    Ok(())
}
