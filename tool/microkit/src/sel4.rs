//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//
use std::cmp::max;

use serde::Deserialize;

use crate::{elf::ElfFile, util, DisjointMemoryRegion, MemoryRegion, UntypedObject};

pub struct KernelPartialBootInfo {
    device_memory: DisjointMemoryRegion,
    normal_memory: DisjointMemoryRegion,
    boot_region: MemoryRegion,
}

#[derive(Clone, Debug)]
pub struct BootInfo {
    pub fixed_cap_count: u64,
    pub sched_control_cap: u64,
    pub paging_cap_count: u64,
    pub page_cap_count: u64,
    pub untyped_objects: Vec<UntypedObject>,
    pub first_available_cap: u64,
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
/// where the reserved region is allocated to determine the memory ranges
/// available. Only valid for ARM and RISC-V platforms.
///
fn kernel_partial_boot(kernel_config: &Config, kernel_elf: &ElfFile) -> KernelPartialBootInfo {
    // Determine the untyped caps of the system
    // This lets allocations happen correctly.
    let mut device_memory = DisjointMemoryRegion::default();
    let mut normal_memory = DisjointMemoryRegion::default();

    for r in kernel_config.device_regions.as_ref().unwrap().iter() {
        device_memory.insert_region(r.start, r.end);
    }
    for r in kernel_config.normal_regions.as_ref().unwrap().iter() {
        normal_memory.insert_region(r.start, r.end);
    }

    // Remove the kernel image itself
    let self_mem = kernel_self_mem(kernel_elf);
    normal_memory.remove_region(self_mem.base, self_mem.end);

    // but get the boot region, we'll add that back later
    // @ivanv: Why calculate it now if we add it back later?
    let boot_region = kernel_boot_mem(kernel_elf);

    KernelPartialBootInfo {
        device_memory,
        normal_memory,
        boot_region,
    }
}

pub fn emulate_kernel_boot_partial(
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

            if config.aarch64_vspace_s2_start_l1() {
                get_n_paging(region, PUD_INDEX_OFFSET) + get_n_paging(region, PD_INDEX_OFFSET)
            } else {
                const PGD_INDEX_OFFSET: u64 = PUD_INDEX_OFFSET + 9;
                get_n_paging(region, PGD_INDEX_OFFSET)
                    + get_n_paging(region, PUD_INDEX_OFFSET)
                    + get_n_paging(region, PD_INDEX_OFFSET)
            }
        }
        Arch::Riscv64 => match config.riscv_pt_levels.unwrap() {
            RiscvVirtualMemory::Sv39 => {
                const PT_INDEX_OFFSET: u64 = 12;
                const LVL1_INDEX_OFFSET: u64 = PT_INDEX_OFFSET + 9;
                const LVL2_INDEX_OFFSET: u64 = LVL1_INDEX_OFFSET + 9;

                get_n_paging(region, LVL2_INDEX_OFFSET) + get_n_paging(region, LVL1_INDEX_OFFSET)
            }
        },
        Arch::X86_64 => unreachable!("the kernel boot process should not be emulated for x86!"),
    }
}

/// Refer to `calculate_rootserver_size()` in src/kernel/boot.c of seL4
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

fn rootserver_max_size_bits(config: &Config) -> u64 {
    let slot_bits = 5; // seL4_SlotBits
    let root_cnode_bits = config.init_cnode_bits; // CONFIG_ROOT_CNODE_SIZE_BITS
    let vspace_bits = ObjectType::VSpace.fixed_size_bits(config).unwrap();

    let cnode_size_bits = root_cnode_bits + slot_bits;
    max(cnode_size_bits, vspace_bits)
}

/// Emulate what happens during a kernel boot, generating a
/// representation of the BootInfo struct.
pub fn emulate_kernel_boot(
    config: &Config,
    kernel_elf: &ElfFile,
    initial_task_phys_region: MemoryRegion,
    user_image_virt_region: MemoryRegion,
) -> BootInfo {
    assert!(initial_task_phys_region.size() == user_image_virt_region.size());
    let partial_info = kernel_partial_boot(config, kernel_elf);
    let mut normal_memory = partial_info.normal_memory;
    let device_memory = partial_info.device_memory;
    let boot_region = partial_info.boot_region;

    normal_memory.remove_region(initial_task_phys_region.base, initial_task_phys_region.end);

    let mut initial_task_virt_region = user_image_virt_region;
    // Refer to `try_init_kernel()` of src/arch/[arm,riscv]/kernel/boot.c
    let ipc_size = PageSize::Small as u64; // seL4_PageBits
    let bootinfo_size = PageSize::Small as u64; // seL4_BootInfoFrameBits
    initial_task_virt_region.end += ipc_size;
    initial_task_virt_region.end += bootinfo_size;

    // Now, the tricky part! determine which memory is used for the initial task objects
    let initial_objects_size = calculate_rootserver_size(config, initial_task_virt_region);
    let initial_objects_align = rootserver_max_size_bits(config);

    // Find an appropriate region of normal memory to allocate the objects
    // from; this follows the same algorithm used within the kernel boot code
    // (or at least we hope it does!)
    // TODO: this loop could be done better in a functional way?
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
        Arch::X86_64 => unreachable!("the kernel boot process should not be emulated for x86!"),
    };
    let device_regions: Vec<MemoryRegion> =
        [device_memory.aligned_power_of_two_regions(config, max_bits)].concat();
    let normal_regions: Vec<MemoryRegion> = [
        boot_region.aligned_power_of_two_regions(config, max_bits),
        normal_memory.aligned_power_of_two_regions(config, max_bits),
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

#[derive(Deserialize)]
pub struct PlatformConfigRegion {
    pub start: u64,
    pub end: u64,
}

#[derive(Deserialize)]
pub struct PlatformConfig {
    pub devices: Vec<PlatformConfigRegion>,
    pub memory: Vec<PlatformConfigRegion>,
}

pub struct Config {
    pub arch: Arch,
    pub word_size: u64,
    pub minimum_page_size: u64,
    pub paddr_user_device_top: u64,
    pub kernel_frame_size: u64,
    pub init_cnode_bits: u64,
    pub cap_address_bits: u64,
    pub fan_out_limit: u64,
    pub max_num_bootinfo_untypeds: u64,
    pub hypervisor: bool,
    pub benchmark: bool,
    pub fpu: bool,
    /// ARM-specific, number of physical address bits
    pub arm_pa_size_bits: Option<usize>,
    /// ARM-specific, where or not SMC forwarding is allowed
    /// False if the kernel config option has not been enabled.
    /// None on any non-ARM architecture.
    pub arm_smc: Option<bool>,
    /// RISC-V specific, what kind of virtual memory system (e.g Sv39)
    pub riscv_pt_levels: Option<RiscvVirtualMemory>,
    /// x86 specific, user context size
    pub x86_xsave_size: Option<usize>,
    pub invocations_labels: serde_json::Value,
    /// The two remaining fields are only valid on ARM and RISC-V
    pub device_regions: Option<Vec<PlatformConfigRegion>>,
    pub normal_regions: Option<Vec<PlatformConfigRegion>>,
}

impl Config {
    pub fn user_top(&self) -> u64 {
        match self.arch {
            Arch::Aarch64 => match self.hypervisor {
                true => match self.arm_pa_size_bits.unwrap() {
                    40 => 0x10000000000,
                    44 => 0x100000000000,
                    _ => panic!("Unknown ARM physical address size bits"),
                },
                false => 0x800000000000,
            },
            Arch::Riscv64 => 0x0000003ffffff000,
            Arch::X86_64 => 0x7ffffffff000,
        }
    }

    pub fn virtual_base(&self) -> u64 {
        match self.arch {
            Arch::Aarch64 => match self.hypervisor {
                true => 0x0000008000000000,
                false => u64::pow(2, 64) - u64::pow(2, 39),
            },
            Arch::Riscv64 => match self.riscv_pt_levels.unwrap() {
                RiscvVirtualMemory::Sv39 => u64::pow(2, 64) - u64::pow(2, 38),
            },
            Arch::X86_64 => u64::pow(2, 64) - u64::pow(2, 39),
        }
    }

    pub fn page_sizes(&self) -> [u64; 2] {
        match self.arch {
            Arch::Aarch64 | Arch::Riscv64 | Arch::X86_64 => [0x1000, 0x200_000],
        }
    }

    pub fn pd_stack_top(&self) -> u64 {
        self.user_top()
    }

    pub fn pd_stack_bottom(&self, stack_size: u64) -> u64 {
        self.pd_stack_top() - stack_size
    }

    /// For simplicity and consistency, the stack of each PD occupies the highest
    /// possible virtual memory region. That means that the highest possible address
    /// for a user to be able to create a mapping at is below the stack region.
    pub fn pd_map_max_vaddr(&self, stack_size: u64) -> u64 {
        // This function depends on the invariant that the stack of a PD
        // consumes the highest possible address of the virtual address space.
        assert!(self.pd_stack_top() == self.user_top());

        self.pd_stack_bottom(stack_size)
    }

    /// Unlike PDs, virtual machines do not have a stack and so the max virtual
    /// address of a mapping is whatever seL4 chooses as the maximum virtual address
    /// in a VSpace.
    pub fn vm_map_max_vaddr(&self) -> u64 {
        self.user_top()
    }

    pub fn paddr_to_kernel_vaddr(&self, paddr: u64) -> u64 {
        paddr.wrapping_add(self.virtual_base())
    }

    pub fn kernel_vaddr_to_paddr(&self, vaddr: u64) -> u64 {
        vaddr.wrapping_sub(self.virtual_base())
    }

    pub fn aarch64_vspace_s2_start_l1(&self) -> bool {
        match self.arch {
            Arch::Aarch64 => self.hypervisor && self.arm_pa_size_bits.unwrap() == 40,
            _ => panic!("internal error"),
        }
    }

    pub fn num_page_table_levels(&self) -> usize {
        match self.arch {
            Arch::Aarch64 => 4,
            Arch::Riscv64 => self.riscv_pt_levels.unwrap().levels(),
            // seL4 only supports 4-level page table on x86-64.
            Arch::X86_64 => 4,
        }
    }
}

#[derive(PartialEq, Eq)]
pub enum Arch {
    Aarch64,
    Riscv64,
    X86_64,
}

/// RISC-V supports multiple virtual memory systems and so we use this enum
/// to make it easier to support more virtual memory systems in the future.
#[derive(Debug, Copy, Clone)]
pub enum RiscvVirtualMemory {
    Sv39,
}

impl RiscvVirtualMemory {
    /// Returns number of page-table levels for a particular virtual memory system.
    pub fn levels(self) -> usize {
        match self {
            RiscvVirtualMemory::Sv39 => 3,
        }
    }
}

#[derive(Debug, Hash, Eq, PartialEq, Clone)]
pub enum ObjectType {
    Untyped,
    Tcb,
    Endpoint,
    Notification,
    CNode,
    SchedContext,
    Reply,
    HugePage,
    VSpace,
    SmallPage,
    LargePage,
    PageTable,
    Vcpu,
    AsidPool,
}

impl ObjectType {
    /// Gets the number of bits to represent the size of a object. The
    /// size depends on architecture as well as kernel configuration.
    pub fn fixed_size_bits(self, config: &Config) -> Option<u64> {
        match self {
            ObjectType::Tcb => match config.arch {
                Arch::Aarch64 => Some(11),
                Arch::Riscv64 => match config.fpu {
                    true => Some(11),
                    false => Some(10),
                },
                Arch::X86_64 => {
                    // matches seL4/libsel4/sel4_arch_include/x86_64/sel4/sel4_arch/constants.h
                    if config.x86_xsave_size.unwrap() >= 832 {
                        Some(12)
                    } else {
                        Some(11)
                    }
                }
            },
            ObjectType::Endpoint => Some(4),
            ObjectType::Notification => Some(6),
            ObjectType::Reply => Some(5),
            ObjectType::VSpace => match config.arch {
                Arch::Aarch64 => match config.hypervisor {
                    true => match config.arm_pa_size_bits.unwrap() {
                        40 => Some(13),
                        44 => Some(12),
                        _ => {
                            panic!("Unexpected ARM PA size bits when determining VSpace size bits")
                        }
                    },
                    false => Some(12),
                },
                _ => Some(12),
            },
            ObjectType::PageTable => Some(12),
            ObjectType::HugePage => Some(30),
            ObjectType::LargePage => Some(21),
            ObjectType::SmallPage => Some(12),
            ObjectType::Vcpu => match config.arch {
                Arch::Aarch64 => Some(12),
                Arch::X86_64 => Some(14),
                _ => panic!("Unexpected architecture asking for vCPU size bits"),
            },
            ObjectType::AsidPool => Some(12),
            _ => None,
        }
    }

    pub fn fixed_size(self, config: &Config) -> Option<u64> {
        self.fixed_size_bits(config).map(|bits| 1 << bits)
    }
}

#[repr(u64)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum PageSize {
    Small = 0x1000,
    Large = 0x200_000,
}

impl From<u64> for PageSize {
    fn from(item: u64) -> PageSize {
        match item {
            0x1000 => PageSize::Small,
            0x200_000 => PageSize::Large,
            _ => panic!("Unknown page size {item:x}"),
        }
    }
}

impl PageSize {
    pub fn fixed_size_bits(&self, sel4_config: &Config) -> u64 {
        match self {
            PageSize::Small => ObjectType::SmallPage.fixed_size_bits(sel4_config).unwrap(),
            PageSize::Large => ObjectType::LargePage.fixed_size_bits(sel4_config).unwrap(),
        }
    }
}

// @merge: I would rather have the duplication of ARM and RISC-V
// rather than a type that tries to unify both.
#[repr(u64)]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
/// The same values apply to ARM and RISC-V
pub enum ArmRiscvIrqTrigger {
    Level = 0,
    Edge = 1,
}

impl From<u64> for ArmRiscvIrqTrigger {
    fn from(item: u64) -> ArmRiscvIrqTrigger {
        match item {
            0 => ArmRiscvIrqTrigger::Level,
            1 => ArmRiscvIrqTrigger::Edge,
            _ => panic!("Unknown ARM/RISC-V IRQ trigger {item:x}"),
        }
    }
}

impl ArmRiscvIrqTrigger {
    pub fn human_name(&self) -> &str {
        match self {
            ArmRiscvIrqTrigger::Level => "level",
            ArmRiscvIrqTrigger::Edge => "edge",
        }
    }
}

#[repr(u64)]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum X86IoapicIrqTrigger {
    Level = 1,
    Edge = 0,
}

impl From<u64> for X86IoapicIrqTrigger {
    fn from(item: u64) -> X86IoapicIrqTrigger {
        match item {
            0 => X86IoapicIrqTrigger::Edge,
            1 => X86IoapicIrqTrigger::Level,
            _ => panic!("Unknown x86 IOAPIC IRQ trigger {item:x}"),
        }
    }
}

impl X86IoapicIrqTrigger {
    pub fn human_name(&self) -> &str {
        match self {
            X86IoapicIrqTrigger::Level => "level",
            X86IoapicIrqTrigger::Edge => "edge",
        }
    }
}

#[repr(u64)]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum X86IoapicIrqPolarity {
    LowTriggered = 0,
    HighTriggered = 1,
}

impl From<u64> for X86IoapicIrqPolarity {
    fn from(item: u64) -> X86IoapicIrqPolarity {
        match item {
            0 => X86IoapicIrqPolarity::LowTriggered,
            1 => X86IoapicIrqPolarity::HighTriggered,
            _ => panic!("Unknown x86 IOAPIC IRQ polarity {item:x}"),
        }
    }
}

impl X86IoapicIrqPolarity {
    pub fn human_name(&self) -> &str {
        match self {
            X86IoapicIrqPolarity::LowTriggered => "low-triggered",
            X86IoapicIrqPolarity::HighTriggered => "high-triggered",
        }
    }
}
