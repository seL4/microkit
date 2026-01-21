//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//
use core::ops::Range;

use std::{
    cmp::{min, Ordering},
    collections::HashMap,
};

use sel4_capdl_initializer_types::{
    object, Cap, CapTableEntry, Fill, FillEntry, FillEntryContent, NamedObject, Object, ObjectId,
    Spec, Word,
};

use crate::{
    capdl::{
        irq::create_irq_handler_cap,
        memory::{create_vspace, create_vspace_ept, map_page},
        spec::{capdl_obj_physical_size_bits, ElfContent},
        util::*,
    },
    elf::ElfFile,
    sdf::{
        CapMapType, CpuCore, SysMap, SysMapPerms, SystemDescription, BUDGET_DEFAULT,
        MONITOR_PD_NAME, MONITOR_PRIORITY,
    },
    sel4::{Arch, Config, PageSize},
    util::{ranges_overlap, round_down, round_up},
};

// Corresponds to the IPC buffer symbol in libmicrokit and the monitor
const SYMBOL_IPC_BUFFER: &str = "__sel4_ipc_buffer_obj";

const FAULT_BADGE: u64 = 1 << 62;
const PPC_BADGE: u64 = 1 << 63;

// The sel4-capdl-initialiser crate expects caps that you want to bind to a TCB to be at
// certain slots. From dep/rust-sel4/crates/sel4-capdl-initializer/types/src/cap_table.rs
pub enum TcbBoundSlot {
    CSpace = 0,
    VSpace = 1,
    IpcBuffer = 4,
    FaultEp = 5,
    SchedContext = 6,
    BoundNotification = 8,
    VCpu = 9,
    // Guest VM root page table object on x86
    X86Eptpml4 = 10,
}

impl From<u32> for TcbBoundSlot {
    fn from(value: u32) -> Self {
        match value {
            0 => Self::CSpace,
            1 => Self::VSpace,
            4 => Self::IpcBuffer,
            5 => Self::FaultEp,
            6 => Self::SchedContext,
            8 => Self::BoundNotification,
            9 => Self::VCpu,
            10 => Self::X86Eptpml4,
            _ => unreachable!("internal bug: unknown value for TcbBoundSlot::from"),
        }
    }
}

const MON_STACK_SIZE: u64 = 0x1000;

// Where caps must be in the Monitor's CSpace
const MON_FAULT_EP_CAP_IDX: u64 = 1;
const MON_REPLY_CAP_IDX: u64 = 2;
const MON_BASE_PD_TCB_CAP: u64 = 10;
const MON_BASE_VM_TCB_CAP: u64 = MON_BASE_PD_TCB_CAP + 64;
const MON_BASE_SCHED_CONTEXT_CAP: u64 = MON_BASE_VM_TCB_CAP + 64;
const MON_BASE_NOTIFICATION_CAP: u64 = MON_BASE_SCHED_CONTEXT_CAP + 64;

// Where caps must be in a PD's CSpace
const PD_INPUT_CAP_IDX: u64 = 1;
const PD_FAULT_EP_CAP_IDX: u64 = 2;
const PD_VSPACE_CAP_IDX: u64 = 3;
const PD_REPLY_CAP_IDX: u64 = 4;
// Valid only if the PD is passive.
const PD_MONITOR_EP_CAP_IDX: u64 = 5;
// Valid only in benchmark configuration.
const PD_TCB_CAP_IDX: u64 = 6;
const PD_ARM_SMC_CAP_IDX: u64 = 7;

const PD_BASE_OUTPUT_NOTIFICATION_CAP: u64 = 10;
const PD_BASE_OUTPUT_ENDPOINT_CAP: u64 = PD_BASE_OUTPUT_NOTIFICATION_CAP + 64;
const PD_BASE_IRQ_CAP: u64 = PD_BASE_OUTPUT_ENDPOINT_CAP + 64;
const PD_BASE_PD_TCB_CAP: u64 = PD_BASE_IRQ_CAP + 64;
const PD_BASE_PD_SC_CAP: u64 = PD_BASE_PD_TCB_CAP + 64;
const PD_BASE_VM_TCB_CAP: u64 = PD_BASE_PD_SC_CAP + 64;
const PD_BASE_VCPU_CAP: u64 = PD_BASE_VM_TCB_CAP + 64;
const PD_BASE_IOPORT_CAP: u64 = PD_BASE_VCPU_CAP + 64;
// The following region can be used for whatever the user wants to map into their
// cspace. We restrict them to use this region so that they don't accidently
// overwrite other parts of the cspace. The cspace slot that the users provide
// for mapping in extra caps such as TCBs and SCs will be as an offset to this
// index. We are bounding this to 128 slots for now.
const PD_BASE_USER_CAPS: u64 = PD_BASE_IOPORT_CAP + 64;

pub const PD_CAP_SIZE: u32 = 1024;
const PD_CAP_BITS: u8 = PD_CAP_SIZE.ilog2() as u8;
const PD_SCHEDCONTEXT_EXTRA_SIZE: u64 = 256;
const PD_SCHEDCONTEXT_EXTRA_SIZE_BITS: u64 = PD_SCHEDCONTEXT_EXTRA_SIZE.ilog2() as u64;

pub const SLOT_BITS: u64 = 5;
pub const SLOT_SIZE: u64 = 1 << SLOT_BITS;

pub type FrameFill = Fill<ElfContent>;
pub type CapDLNamedObject = NamedObject<FrameFill>;

pub struct ExpectedAllocation {
    pub ut_idx: usize,
    pub paddr: u64,
}

pub struct CapDLSpecContainer {
    pub spec: Spec<FrameFill>,
    /// Track allocations as we build the system for later use by the report.
    pub expected_allocations: HashMap<ObjectId, ExpectedAllocation>,
}

impl Default for CapDLSpecContainer {
    fn default() -> Self {
        Self::new()
    }
}

impl CapDLSpecContainer {
    pub fn new() -> Self {
        Self {
            spec: Spec {
                objects: Vec::new(),
                irqs: Vec::new(),
                asid_slots: Vec::new(),
                root_objects: Range {
                    start: 0.into(),
                    end: 0.into(),
                },
                untyped_covers: Vec::new(),
                cached_orig_cap_slots: None,
                log_level: None,
            },
            expected_allocations: HashMap::new(),
        }
    }

    pub fn add_root_object(&mut self, obj: CapDLNamedObject) -> ObjectId {
        self.spec.objects.push(obj);
        self.spec.root_objects.end = (usize::from(self.spec.root_objects.end) + 1).into();
        assert_eq!(
            self.spec.objects.len(),
            usize::from(self.spec.root_objects.end)
        );
        (usize::from(self.spec.root_objects.end) - 1).into()
    }

    pub fn get_root_object_mut(&mut self, obj_id: ObjectId) -> Option<&mut CapDLNamedObject> {
        if usize::from(obj_id) < usize::from(self.spec.root_objects.end) {
            Some(&mut self.spec.objects[usize::from(obj_id)])
        } else {
            None
        }
    }

    pub fn get_root_object(&self, obj_id: ObjectId) -> Option<&CapDLNamedObject> {
        if usize::from(obj_id) < usize::from(self.spec.root_objects.end) {
            Some(&self.spec.objects[usize::from(obj_id)])
        } else {
            None
        }
    }

    /// Add the details of the given ELF into the given CapDL spec while inferring as much information
    /// as possible. These are the objects that will be created:
    /// -> TCB: PC, SP and IPC buffer vaddr set. VSpace and IPC buffer frame caps bound.
    /// -> VSpace: all ELF loadable pages and IPC buffer mapped in.
    /// Returns the object ID of the TCB
    /// NOTE that all ELF frames will just be reference to the original ELF object rather than the actual data.
    /// So that symbols can be patched before the frames' data are filled in.
    fn add_elf_to_spec(
        &mut self,
        sel4_config: &Config,
        pd_name: &str,
        pd_cpu: CpuCore,
        elf_id: usize,
        elf: &ElfFile,
    ) -> Result<ObjectId, String> {
        // We assumes that ELFs and PDs have a one-to-one relationship. So for each ELF we create a VSpace.
        let vspace_obj_id = create_vspace(self, sel4_config, pd_name);
        let vspace_cap = capdl_util_make_page_table_cap(vspace_obj_id);

        // For each loadable segment in the ELF, map it into the address space of this PD.
        let mut frame_sequence = 0; // For object naming purpose only.
        for (seg_idx, segment) in elf.loadable_segments().iter().enumerate() {
            if segment.data().is_empty() {
                continue;
            }

            let seg_base_vaddr = segment.virt_addr;
            let seg_mem_size: u64 = segment.mem_size();

            let page_size = PageSize::Small;
            let page_size_bytes = page_size as u64;

            // Create and map all frames for this segment.
            let mut cur_vaddr = round_down(seg_base_vaddr, page_size_bytes);
            while cur_vaddr < seg_base_vaddr + seg_mem_size {
                let mut frame_fill = Fill {
                    entries: [].to_vec(),
                };

                // Now compute the ELF file offset to fill in this page.
                let mut dest_offset = 0;
                if cur_vaddr < seg_base_vaddr {
                    // Take care of case where the ELF segment is not aligned on page boundary:
                    //     |   ELF    |   ELF    |   ELF    |
                    // |   Page   |   Page   |   Page   |
                    //  <->
                    dest_offset = seg_base_vaddr - cur_vaddr;
                }

                let target_vaddr_start = cur_vaddr + dest_offset;
                let section_offset = target_vaddr_start - seg_base_vaddr;
                if section_offset < seg_mem_size {
                    // We have data to load
                    let len_to_cpy =
                        min(page_size_bytes - dest_offset, seg_mem_size - section_offset);

                    frame_fill.entries.push(FillEntry {
                        range: Range {
                            start: dest_offset,
                            end: dest_offset + len_to_cpy,
                        },
                        content: FillEntryContent::Data(ElfContent {
                            elf_id,
                            elf_seg_idx: seg_idx,
                            elf_seg_data_range: (section_offset as usize
                                ..((section_offset + len_to_cpy) as usize)),
                        }),
                    });
                }

                // Create the frame object, cap to the object, add it to the spec and map it in.
                let frame_obj_id = capdl_util_make_frame_obj(
                    self,
                    frame_fill,
                    &format!("elf_{pd_name}_{frame_sequence:09}"),
                    None,
                    PageSize::Small.fixed_size_bits(sel4_config) as u8,
                );
                let frame_cap = capdl_util_make_frame_cap(
                    frame_obj_id,
                    segment.is_readable(),
                    segment.is_writable(),
                    segment.is_executable(),
                    true,
                );

                match map_page(
                    self,
                    sel4_config,
                    pd_name,
                    vspace_obj_id,
                    frame_cap,
                    page_size_bytes,
                    cur_vaddr,
                ) {
                    Ok(_) => {
                        frame_sequence += 1;
                        cur_vaddr += page_size_bytes;
                    }
                    Err(map_err_reason) => {
                        return Err(format!(
                            "add_elf_to_spec(): failed to map segment page to ELF because: {map_err_reason}"
                        ))
                    }
                };
            }
        }

        // Create and map the IPC buffer for this ELF
        let ipcbuf_frame_obj_id = capdl_util_make_frame_obj(
            self,
            Fill {
                entries: [].to_vec(),
            },
            &format!("ipcbuf_{pd_name}"),
            None,
            PageSize::Small.fixed_size_bits(sel4_config) as u8,
        );
        let ipcbuf_frame_cap =
            capdl_util_make_frame_cap(ipcbuf_frame_obj_id, true, true, false, true);
        // We need to clone the IPC buf cap because in addition to mapping the frame into the VSpace, we need to bind
        // this frame to the TCB as well.
        let ipcbuf_frame_cap_for_tcb = ipcbuf_frame_cap.clone();
        let ipcbuf_vaddr = elf
            .find_symbol(SYMBOL_IPC_BUFFER)
            .unwrap_or_else(|_| panic!("Could not find {SYMBOL_IPC_BUFFER}"))
            .0;
        match map_page(
            self,
            sel4_config,
            pd_name,
            vspace_obj_id,
            ipcbuf_frame_cap,
            PageSize::Small as u64,
            ipcbuf_vaddr,
        ) {
            Ok(_) => {}
            Err(map_err_reason) => {
                return Err(format!(
                    "build_capdl_spec(): failed to map ipc buffer frame to {pd_name} because: {map_err_reason}"
                ))
            }
        };

        let tcb_name = format!("tcb_{pd_name}");
        let entry_point = elf.entry;

        let tcb_extra_info = object::TcbExtraInfo {
            ipc_buffer_addr: ipcbuf_vaddr.into(),
            affinity: Word(pd_cpu.0.into()),
            prio: 0,
            max_prio: 0,
            resume: false,
            ip: entry_point.into(),
            sp: 0.into(),
            gprs: Vec::new(),
            master_fault_ep: None,
        };

        let tcb_inner_obj = object::Tcb {
            // Bind the VSpace into the TCB
            slots: [
                capdl_util_make_cte(TcbBoundSlot::VSpace as u32, vspace_cap),
                capdl_util_make_cte(TcbBoundSlot::IpcBuffer as u32, ipcbuf_frame_cap_for_tcb),
            ]
            .to_vec(),
            extra: Box::new(tcb_extra_info),
        };

        let tcb_obj = NamedObject {
            name: Some(tcb_name),
            object: Object::Tcb(tcb_inner_obj),
        };

        Ok(self.add_root_object(tcb_obj))
    }
}

/// Given a SysMap, page size, VSpace object ID, and a Vec of frame object ids,
/// map all frames into the given VSpace at the requested vaddr.
fn map_memory_region(
    spec_container: &mut CapDLSpecContainer,
    sel4_config: &Config,
    pd_name: &str,
    map: &SysMap,
    page_sz: u64,
    target_vspace: ObjectId,
    frames: &[ObjectId],
) {
    let mut cur_vaddr = map.vaddr;
    let read = map.perms & SysMapPerms::Read as u8 != 0;
    let write = map.perms & SysMapPerms::Write as u8 != 0;
    let execute = map.perms & SysMapPerms::Execute as u8 != 0;
    let cached = map.cached;
    for frame_obj_id in frames.iter() {
        // Make a cap for this frame.
        let frame_cap = capdl_util_make_frame_cap(*frame_obj_id, read, write, execute, cached);
        // Map it into this PD address space.
        map_page(
            spec_container,
            sel4_config,
            pd_name,
            target_vspace,
            frame_cap,
            page_sz,
            cur_vaddr,
        )
        .unwrap();
        cur_vaddr += page_sz;
    }
}

/// Build a CapDL Spec according to the System Description File.
pub fn build_capdl_spec(
    kernel_config: &Config,
    elfs: &mut [ElfFile],
    system: &SystemDescription,
) -> Result<CapDLSpecContainer, String> {
    let mut spec_container = CapDLSpecContainer::new();

    // *********************************
    // Step 1. Create the monitor's spec.
    // *********************************
    // Parse ELF, create VSpace, map in all ELF loadable frames and IPC buffer, and create TCB.
    // We expect the PD ELFs to be first and the monitor ELF last in the list of ELFs.
    let mon_elf_id = elfs.len() - 1;
    assert!(elfs.len() == system.protection_domains.len() + 1);
    let monitor_tcb_obj_id = {
        let monitor_elf = elfs.get(mon_elf_id).unwrap();
        spec_container
            .add_elf_to_spec(
                kernel_config,
                MONITOR_PD_NAME,
                CpuCore(0),
                mon_elf_id,
                monitor_elf,
            )
            .unwrap()
    };

    // Create monitor fault endpoint object + cap
    let mon_fault_ep_obj_id =
        capdl_util_make_endpoint_obj(&mut spec_container, MONITOR_PD_NAME, true);
    let mon_fault_ep_cap = capdl_util_make_endpoint_cap(mon_fault_ep_obj_id, true, true, true, 0);

    // Create monitor reply object object + cap
    let mon_reply_obj_id = capdl_util_make_reply_obj(&mut spec_container, MONITOR_PD_NAME);
    let mon_reply_cap = capdl_util_make_reply_cap(mon_reply_obj_id);

    // Create monitor scheduling context object + cap
    let mon_sc_obj_id = capdl_util_make_sc_obj(
        &mut spec_container,
        MONITOR_PD_NAME,
        PD_SCHEDCONTEXT_EXTRA_SIZE_BITS as u8,
        BUDGET_DEFAULT,
        BUDGET_DEFAULT,
        0,
    );
    let mon_sc_cap = capdl_util_make_sc_cap(mon_sc_obj_id);

    // Create monitor CSpace and pre-insert the fault EP and reply caps into the correct slots in CSpace.
    let mon_cnode_obj_id = capdl_util_make_cnode_obj(
        &mut spec_container,
        MONITOR_PD_NAME,
        PD_CAP_BITS,
        [
            capdl_util_make_cte(MON_FAULT_EP_CAP_IDX as u32, mon_fault_ep_cap),
            capdl_util_make_cte(MON_REPLY_CAP_IDX as u32, mon_reply_cap),
        ]
        .to_vec(),
    );
    let mon_guard_size = kernel_config.cap_address_bits - PD_CAP_BITS as u64;
    let mon_cnode_cap = capdl_util_make_cnode_cap(mon_cnode_obj_id, 0, mon_guard_size as u8);

    // Create monitor stack frame
    let mon_stack_frame_obj_id = capdl_util_make_frame_obj(
        &mut spec_container,
        Fill {
            entries: [].to_vec(),
        },
        &format!("{MONITOR_PD_NAME}_stack"),
        None,
        PageSize::Small.fixed_size_bits(kernel_config) as u8,
    );
    let mon_stack_frame_cap =
        capdl_util_make_frame_cap(mon_stack_frame_obj_id, true, true, false, true);
    let mon_vspace_obj_id =
        capdl_util_get_vspace_id_from_tcb_id(&spec_container, monitor_tcb_obj_id);
    map_page(
        &mut spec_container,
        kernel_config,
        MONITOR_PD_NAME,
        mon_vspace_obj_id,
        mon_stack_frame_cap,
        PageSize::Small as u64,
        kernel_config.pd_stack_bottom(MON_STACK_SIZE),
    )
    .unwrap();

    // At this point, all of the required objects for the monitor have been created and it caps inserted into
    // the correct slot in the CSpace. We need to bind those objects into the TCB for the monitor to use them.
    // In addition, `add_elf_to_spec()` doesn't fill most the details in the TCB.
    // Now fill them in: stack ptr, priority, ipc buf vaddr, etc.
    if let Object::Tcb(monitor_tcb) = &mut spec_container
        .get_root_object_mut(monitor_tcb_obj_id)
        .unwrap()
        .object
    {
        // Special case, monitor has its stack statically allocated.
        monitor_tcb.extra.sp = Word(kernel_config.pd_stack_top());
        // While there is nothing stopping us from running the monitor at the highest priority alongside the
        // CapDL initialiser, the debug kernel serial output can get garbled when the monitor TCB is resumed.
        monitor_tcb.extra.prio = MONITOR_PRIORITY;
        monitor_tcb.extra.max_prio = MONITOR_PRIORITY;
        monitor_tcb.extra.resume = true;

        monitor_tcb.slots.push(capdl_util_make_cte(
            TcbBoundSlot::CSpace as u32,
            mon_cnode_cap,
        ));

        monitor_tcb.slots.push(capdl_util_make_cte(
            TcbBoundSlot::SchedContext as u32,
            mon_sc_cap,
        ));
    } else {
        unreachable!("internal bug: build_capdl_spec() got a non TCB object ID when trying to set TCB parameters for the monitor.");
    }

    // *********************************
    // Step 2. Create the memory regions' spec. Result is a hashmap keyed on MR name, value is (parsed XML obj, Vec of frame object IDs)
    // *********************************
    let mut mr_name_to_frames: HashMap<&String, Vec<ObjectId>> = HashMap::new();
    for mr in system.memory_regions.iter() {
        let mut frame_ids = Vec::new();
        let frame_size_bits = mr.page_size.fixed_size_bits(kernel_config);

        for frame_sequence in 0..mr.page_count {
            let paddr = mr
                .paddr()
                .map(|base_paddr| Word(base_paddr + (frame_sequence * mr.page_size_bytes())));
            frame_ids.push(capdl_util_make_frame_obj(
                &mut spec_container,
                Fill {
                    entries: [].to_vec(),
                },
                &format!("mr_{}_{:09}", mr.name, frame_sequence),
                paddr,
                frame_size_bits as u8,
            ));
        }

        mr_name_to_frames.insert(&mr.name, frame_ids);
    }

    // *********************************
    // Step 3. Create the PDs' spec
    // *********************************
    // On ARM, check if we need to create the SMC object
    let arm_smc_obj_id = if kernel_config.arch == Arch::Aarch64
        && kernel_config.arm_smc.unwrap_or(false)
        && system.protection_domains.iter().any(|pd| pd.smc)
    {
        Some(spec_container.add_root_object(NamedObject {
            name: "arm_smc".to_owned().into(),
            object: Object::ArmSmc,
        }))
    } else {
        None
    };

    // Mapping between pd name and id for faster lookups
    let mut pd_name_to_id: HashMap<String, usize> = HashMap::new();

    // Keep tabs on each PD's CSpace, Notification and Endpoint objects so we can create channels between them at a later step.
    let mut pd_id_to_cspace_id: HashMap<usize, ObjectId> = HashMap::new();
    let mut pd_id_to_ntfn_id: HashMap<usize, ObjectId> = HashMap::new();
    let mut pd_id_to_ep_id: HashMap<usize, ObjectId> = HashMap::new();

    // Keep tabs on caps such as TCB and SC so that we can create additional mappings for the cap into other PD's cspaces.
    let mut pd_shadow_cspace: HashMap<usize, Vec<Option<Cap>>> = HashMap::new();

    // Keep track of the global count of vCPU objects so we can bind them to the monitor for setting TCB name in debug config.
    // Only used on ARM and RISC-V as on x86-64 VMs share the same TCB as PD's which will have their TCB name set separately.
    let mut monitor_vcpu_idx = 0;

    // Keep tabs on each PD's stack bottom so we can write it out to the monitor for stack overflow detection.
    let mut pd_stack_bottoms: Vec<u64> = Vec::new();

    for (pd_global_idx, pd) in system.protection_domains.iter().enumerate() {
        let elf_obj = &elfs[pd_global_idx];

        pd_name_to_id.insert(pd.name.clone(), pd_global_idx);

        let mut caps_to_bind_to_tcb: Vec<CapTableEntry> = Vec::new();
        let mut caps_to_insert_to_pd_cspace: Vec<CapTableEntry> = Vec::new();

        // Step 3-1: Create TCB and VSpace with all ELF loadable frames mapped in.
        let pd_tcb_obj_id = spec_container
            .add_elf_to_spec(kernel_config, &pd.name, pd.cpu, pd_global_idx, elf_obj)
            .unwrap();
        let pd_vspace_obj_id = capdl_util_get_vspace_id_from_tcb_id(&spec_container, pd_tcb_obj_id);

        let pd_tcb_obj = capdl_util_make_tcb_cap(pd_tcb_obj_id);
        let pd_vspace_obj = capdl_util_make_page_table_cap(pd_vspace_obj_id);

        pd_shadow_cspace
            .entry(pd_global_idx)
            .or_insert_with(|| vec![None; CapMapType::__Len as usize])[CapMapType::Tcb as usize] =
            Some(pd_tcb_obj.clone());
        pd_shadow_cspace.get_mut(&pd_global_idx).unwrap()[CapMapType::Vspace as usize] =
            Some(pd_vspace_obj.clone());

        // In the benchmark configuration, we allow PDs to access their own TCB.
        // This is necessary for accessing kernel's benchmark API.
        if kernel_config.benchmark {
            caps_to_insert_to_pd_cspace
                .push(capdl_util_make_cte(PD_TCB_CAP_IDX as u32, pd_tcb_obj));
        }

        // Allow PD to access their own VSpace for ops such as cache cleaning on ARM.
        caps_to_insert_to_pd_cspace.push(capdl_util_make_cte(
            PD_VSPACE_CAP_IDX as u32,
            pd_vspace_obj,
        ));

        // Step 3-2: Map in all Memory Regions
        for map in pd.maps.iter() {
            let frames = mr_name_to_frames.get(&map.mr).unwrap();
            // MRs have frames of equal size so just use the first frame's page size.
            let page_size_bytes =
                1 << capdl_util_get_frame_size_bits(&spec_container, *frames.first().unwrap());

            // sdf.rs sanity checks that the memory regions doesn't overlap with each others, etc.
            // But it doesn't actually check for whether they overlap with a PD's stack or ELF segments.
            // We perform this check here, otherwise the tool will panic with quite cryptic page-table related errors.
            let mr_vaddr_range = map.vaddr..(map.vaddr + (page_size_bytes * frames.len() as u64));

            let pd_stack_range =
                kernel_config.pd_stack_bottom(pd.stack_size)..kernel_config.pd_stack_top();
            if ranges_overlap(&mr_vaddr_range, &pd_stack_range) {
                return Err(format!("ERROR: mapping MR '{}' to PD '{}' with vaddr [0x{:x}..0x{:x}) will overlap with the stack at [0x{:x}..0x{:x})", map.mr, pd.name, mr_vaddr_range.start, mr_vaddr_range.end, pd_stack_range.start, pd_stack_range.end));
            }

            for elf_seg in elf_obj.loadable_segments().iter() {
                let elf_seg_vaddr_range = elf_seg.virt_addr
                    ..elf_seg.virt_addr + round_up(elf_seg.mem_size(), PageSize::Small as u64);
                if ranges_overlap(&mr_vaddr_range, &elf_seg_vaddr_range) {
                    return Err(format!("ERROR: mapping MR '{}' to PD '{}' with vaddr [0x{:x}..0x{:x}) will overlap with an ELF segment at [0x{:x}..0x{:x})", map.mr, pd.name, mr_vaddr_range.start, mr_vaddr_range.end, elf_seg_vaddr_range.start, elf_seg_vaddr_range.end));
                }
            }

            map_memory_region(
                &mut spec_container,
                kernel_config,
                &pd.name,
                map,
                page_size_bytes,
                pd_vspace_obj_id,
                frames,
            );
        }

        // Step 3-3: Create and map in the stack (bottom up)
        let mut cur_stack_vaddr = kernel_config.pd_stack_bottom(pd.stack_size);
        pd_stack_bottoms.push(cur_stack_vaddr);
        let num_stack_frames = pd.stack_size / PageSize::Small as u64;
        for stack_frame_seq in 0..num_stack_frames {
            let stack_frame_obj_id = capdl_util_make_frame_obj(
                &mut spec_container,
                Fill {
                    entries: [].to_vec(),
                },
                &format!("{}_stack_{:09}", pd.name, stack_frame_seq),
                None,
                PageSize::Small.fixed_size_bits(kernel_config) as u8,
            );
            let stack_frame_cap =
                capdl_util_make_frame_cap(stack_frame_obj_id, true, true, false, true);
            map_page(
                &mut spec_container,
                kernel_config,
                &pd.name,
                pd_vspace_obj_id,
                stack_frame_cap,
                PageSize::Small as u64,
                cur_stack_vaddr,
            )
            .unwrap();
            cur_stack_vaddr += PageSize::Small as u64;
        }

        // Step 3-4 Create Scheduling Context
        let pd_sc_obj_id = capdl_util_make_sc_obj(
            &mut spec_container,
            &pd.name,
            PD_SCHEDCONTEXT_EXTRA_SIZE_BITS as u8,
            pd.period,
            pd.budget,
            0x100 + pd_global_idx as u64,
        );
        let pd_sc_cap = capdl_util_make_sc_cap(pd_sc_obj_id);

        pd_shadow_cspace.get_mut(&pd_global_idx).unwrap()[CapMapType::Sc as usize] =
            Some(pd_sc_cap.clone());

        caps_to_bind_to_tcb.push(capdl_util_make_cte(
            TcbBoundSlot::SchedContext as u32,
            pd_sc_cap,
        ));

        // Step 3-5 Create fault Endpoint cap to parent/monitor
        let pd_fault_ep_cap = if pd.parent.is_none() {
            // badge = pd_global_idx + 1 because seL4 considers badge = 0 as no badge.
            let badge: u64 = pd_global_idx as u64 + 1;
            capdl_util_make_endpoint_cap(mon_fault_ep_obj_id, true, true, true, badge)
        } else {
            assert!(pd_global_idx > pd.parent.unwrap());
            let badge: u64 = FAULT_BADGE | pd.id.unwrap();
            let parent_ep_obj_id = pd_id_to_ep_id.get(&pd.parent.unwrap()).unwrap();
            let fault_ep_cap =
                capdl_util_make_endpoint_cap(*parent_ep_obj_id, true, true, true, badge);

            // Allow the parent PD to access the child's TCB:
            let parent_cspace_obj_id = pd_id_to_cspace_id.get(&pd.parent.unwrap()).unwrap();
            capdl_util_insert_cap_into_cspace(
                &mut spec_container,
                *parent_cspace_obj_id,
                (PD_BASE_PD_TCB_CAP + pd.id.unwrap()) as u32,
                capdl_util_make_tcb_cap(pd_tcb_obj_id),
            );

            // Allow the parent PD to access the child's SC:
            capdl_util_insert_cap_into_cspace(
                &mut spec_container,
                *parent_cspace_obj_id,
                (PD_BASE_PD_SC_CAP + pd.id.unwrap()) as u32,
                capdl_util_make_sc_cap(pd_sc_obj_id),
            );

            fault_ep_cap
        };
        caps_to_insert_to_pd_cspace.push(capdl_util_make_cte(
            PD_FAULT_EP_CAP_IDX as u32,
            pd_fault_ep_cap.clone(),
        ));
        caps_to_bind_to_tcb.push(capdl_util_make_cte(
            TcbBoundSlot::FaultEp as u32,
            pd_fault_ep_cap.clone(),
        ));

        // Step 3-6 Create cap to Monitor's endpoint for passive PDs.
        if pd.passive {
            let pd_monitor_ep_cap = capdl_util_make_endpoint_cap(
                mon_fault_ep_obj_id,
                true,
                true,
                true,
                pd_global_idx as u64 + 1,
            );
            caps_to_insert_to_pd_cspace.push(capdl_util_make_cte(
                PD_MONITOR_EP_CAP_IDX as u32,
                pd_monitor_ep_cap,
            ));
        }

        // Step 3-7 Create endpoint object for the PD if it has children or can receive PPCs, else it will be a notification
        let pd_ntfn_obj_id = capdl_util_make_ntfn_obj(&mut spec_container, &pd.name);
        let pd_ntfn_cap = capdl_util_make_ntfn_cap(pd_ntfn_obj_id, true, true, 0);
        let mut pd_ep_obj_id: Option<ObjectId> = None;
        pd_id_to_ntfn_id.insert(pd_global_idx, pd_ntfn_obj_id);
        if pd.needs_ep(pd_global_idx, &system.channels) {
            pd_ep_obj_id = Some(capdl_util_make_endpoint_obj(
                &mut spec_container,
                &pd.name,
                false,
            ));
            let pd_ep_cap =
                capdl_util_make_endpoint_cap(pd_ep_obj_id.unwrap(), true, true, true, 0);
            pd_id_to_ep_id.insert(pd_global_idx, pd_ep_obj_id.unwrap());
            caps_to_insert_to_pd_cspace
                .push(capdl_util_make_cte(PD_INPUT_CAP_IDX as u32, pd_ep_cap));
        } else {
            let pd_ntfn_cap_clone = pd_ntfn_cap.clone();
            caps_to_insert_to_pd_cspace.push(capdl_util_make_cte(
                PD_INPUT_CAP_IDX as u32,
                pd_ntfn_cap_clone,
            ));
        }
        caps_to_bind_to_tcb.push(capdl_util_make_cte(
            TcbBoundSlot::BoundNotification as u32,
            pd_ntfn_cap,
        ));

        // Step 3-8 Create Reply obj + cap and insert into CSpace
        let pd_reply_obj_id = capdl_util_make_reply_obj(&mut spec_container, &pd.name);
        let pd_reply_cap = capdl_util_make_reply_cap(pd_reply_obj_id);
        caps_to_insert_to_pd_cspace
            .push(capdl_util_make_cte(PD_REPLY_CAP_IDX as u32, pd_reply_cap));

        // Step 3-9 Create spec and caps to IRQs
        for irq in pd.irqs.iter() {
            // Create a IRQ handler cap and insert into the requested CSpace's slot.
            let irq_handle_cap = create_irq_handler_cap(
                &mut spec_container,
                kernel_config,
                &pd.name,
                pd.cpu,
                pd_ntfn_obj_id,
                irq,
            );
            let irq_cap_idx = PD_BASE_IRQ_CAP + irq.id;
            caps_to_insert_to_pd_cspace
                .push(capdl_util_make_cte(irq_cap_idx as u32, irq_handle_cap));
        }

        // Step 3-10 Create I/O port objects on x86 platform.
        for ioport in pd.ioports.iter() {
            let ioport_obj_id =
                capdl_util_make_ioport_obj(&mut spec_container, &pd.name, ioport.addr, ioport.size);
            let ioport_cap = capdl_util_make_ioport_cap(ioport_obj_id);
            caps_to_insert_to_pd_cspace.push(capdl_util_make_cte(
                (PD_BASE_IOPORT_CAP + ioport.id) as u32,
                ioport_cap,
            ));
        }

        // Step 3-11 Create VM Spec.
        if let Some(virtual_machine) = &pd.virtual_machine {
            // A VM really is just a collection of special threads, it has its own TCBs, Scheduling Contexts, etc...
            // The difference is that it have a vCPU for each TCB to store the virtual CPUs' states.

            // Create VM's Address Space and map in all memory regions.
            // This address space is shared across all vCPUs. The virtual address that we "map" the region is guest-physical.
            let vm_vspace_obj_id = match kernel_config.arch {
                Arch::X86_64 => {
                    create_vspace_ept(&mut spec_container, kernel_config, &virtual_machine.name)
                }
                _ => create_vspace(&mut spec_container, kernel_config, &virtual_machine.name),
            };
            let vm_vspace_cap = capdl_util_make_page_table_cap(vm_vspace_obj_id);
            for map in virtual_machine.maps.iter() {
                let frames = mr_name_to_frames.get(&map.mr).unwrap();
                let page_size_bytes =
                    1 << capdl_util_get_frame_size_bits(&spec_container, *frames.first().unwrap());
                map_memory_region(
                    &mut spec_container,
                    kernel_config,
                    &virtual_machine.name,
                    map,
                    page_size_bytes,
                    vm_vspace_obj_id,
                    frames,
                );
            }

            if kernel_config.arch == Arch::X86_64 {
                // Only support 1 vcpu on x86 right now.
                assert_eq!(virtual_machine.vcpus.len(), 1);
                let vcpu = virtual_machine.vcpus.first().unwrap();

                // Create the vCPU object and bind it to the VMM PD.
                let vm_vcpu_obj_id = capdl_util_make_vcpu_obj(
                    &mut spec_container,
                    &format!("{}_{}", virtual_machine.name, vcpu.id),
                );
                let vcpu_cap = capdl_util_make_vcpu_cap(vm_vcpu_obj_id);
                caps_to_bind_to_tcb.push(capdl_util_make_cte(
                    TcbBoundSlot::VCpu as u32,
                    vcpu_cap.clone(),
                ));

                // Allow the parent PD to access the vCPU object.
                caps_to_insert_to_pd_cspace.push(capdl_util_make_cte(
                    (PD_BASE_VCPU_CAP + vcpu.id) as u32,
                    vcpu_cap,
                ));

                // Bind the guest's root page table to the parent PD.
                caps_to_bind_to_tcb.push(capdl_util_make_cte(
                    TcbBoundSlot::X86Eptpml4 as u32,
                    vm_vspace_cap,
                ));
            } else {
                for (vcpu_idx, vcpu) in virtual_machine.vcpus.iter().enumerate() {
                    // All vCPUs get to access the same address space.
                    let mut caps_to_bind_to_vm_tcbs: Vec<CapTableEntry> = Vec::new();
                    caps_to_bind_to_vm_tcbs.push(capdl_util_make_cte(
                        TcbBoundSlot::VSpace as u32,
                        vm_vspace_cap.clone(),
                    ));

                    // Create an empty CSpace
                    let vm_cnode_obj_id = capdl_util_make_cnode_obj(
                        &mut spec_container,
                        &format!("{}_{}", virtual_machine.name, vcpu.id),
                        PD_CAP_BITS,
                        [].to_vec(),
                    );
                    let vm_guard_size = kernel_config.cap_address_bits - PD_CAP_BITS as u64;
                    let vm_cnode_cap =
                        capdl_util_make_cnode_cap(vm_cnode_obj_id, 0, vm_guard_size as u8);
                    caps_to_bind_to_vm_tcbs.push(capdl_util_make_cte(
                        TcbBoundSlot::CSpace as u32,
                        vm_cnode_cap,
                    ));

                    // Create and map the IPC buffer.
                    let vm_ipcbuf_frame_obj_id = capdl_util_make_frame_obj(
                        &mut spec_container,
                        Fill {
                            entries: [].to_vec(),
                        },
                        &format!("ipcbuf_{}_{}", virtual_machine.name, vcpu.id),
                        None,
                        // Must be consistent with the granule bits used in spec serialisation
                        PageSize::Small.fixed_size_bits(kernel_config) as u8,
                    );
                    let vm_ipcbuf_frame_cap =
                        capdl_util_make_frame_cap(vm_ipcbuf_frame_obj_id, true, true, false, true);
                    caps_to_bind_to_vm_tcbs.push(capdl_util_make_cte(
                        TcbBoundSlot::IpcBuffer as u32,
                        vm_ipcbuf_frame_cap,
                    ));

                    // Create fault endpoint cap to the parent PD.
                    let vm_vcpu_fault_ep_cap = capdl_util_make_endpoint_cap(
                        pd_ep_obj_id.unwrap(),
                        true,
                        true,
                        true,
                        FAULT_BADGE | vcpu.id,
                    );
                    caps_to_bind_to_vm_tcbs.push(capdl_util_make_cte(
                        TcbBoundSlot::FaultEp as u32,
                        vm_vcpu_fault_ep_cap,
                    ));

                    // Create scheduling context
                    let vm_vcpu_sc_obj_id = capdl_util_make_sc_obj(
                        &mut spec_container,
                        &format!("{}_{}", virtual_machine.name, vcpu.id),
                        PD_SCHEDCONTEXT_EXTRA_SIZE_BITS as u8,
                        virtual_machine.period,
                        virtual_machine.budget,
                        0x100 + vcpu_idx as u64,
                    );
                    caps_to_bind_to_vm_tcbs.push(capdl_util_make_cte(
                        TcbBoundSlot::SchedContext as u32,
                        capdl_util_make_sc_cap(vm_vcpu_sc_obj_id),
                    ));

                    // Create vCPU object
                    let vm_vcpu_obj_id = capdl_util_make_vcpu_obj(
                        &mut spec_container,
                        &format!("{}_{}", virtual_machine.name, vcpu.id),
                    );
                    caps_to_bind_to_vm_tcbs.push(capdl_util_make_cte(
                        TcbBoundSlot::VCpu as u32,
                        capdl_util_make_vcpu_cap(vm_vcpu_obj_id),
                    ));

                    // Finally create TCB, unlike PDs, VMs are suspended by default until resume'd by their parent.
                    let vm_vcpu_tcb_inner_obj = object::Tcb {
                        slots: caps_to_bind_to_vm_tcbs,
                        extra: Box::new(object::TcbExtraInfo {
                            ipc_buffer_addr: Word(0),
                            affinity: Word(vcpu.cpu.0.into()),
                            prio: virtual_machine.priority,
                            max_prio: virtual_machine.priority,
                            resume: false,
                            // VMs do not have program images associated with them so these are always zero.
                            ip: Word(0),
                            sp: Word(0),
                            gprs: [].to_vec(),
                            master_fault_ep: None, // Not used on MCS kernel.
                        }),
                    };
                    let vm_vcpu_tcb_obj_id = spec_container.add_root_object(NamedObject {
                        name: format!("tcb_{}_{}", virtual_machine.name, vcpu.id).into(),
                        object: Object::Tcb(vm_vcpu_tcb_inner_obj),
                    });

                    // Allow parent PD to access this vCPU object and associated TCB
                    caps_to_insert_to_pd_cspace.push(capdl_util_make_cte(
                        (PD_BASE_VCPU_CAP + vcpu.id) as u32,
                        capdl_util_make_vcpu_cap(vm_vcpu_obj_id),
                    ));
                    caps_to_insert_to_pd_cspace.push(capdl_util_make_cte(
                        (PD_BASE_VM_TCB_CAP + vcpu.id) as u32,
                        capdl_util_make_tcb_cap(vm_vcpu_tcb_obj_id),
                    ));

                    // Bind vCPU's TCB to the monitor so that the name can be set at start up in debug config
                    capdl_util_insert_cap_into_cspace(
                        &mut spec_container,
                        mon_cnode_obj_id,
                        (MON_BASE_VM_TCB_CAP as usize + monitor_vcpu_idx) as u32,
                        capdl_util_make_tcb_cap(vm_vcpu_tcb_obj_id),
                    );
                    monitor_vcpu_idx += 1;
                }
            }
        }

        // Step 3-12 Create ARM SMC cap if requested.
        if pd.smc {
            caps_to_insert_to_pd_cspace.push(capdl_util_make_cte(
                PD_ARM_SMC_CAP_IDX as u32,
                capdl_util_make_arm_smc_cap(arm_smc_obj_id.unwrap()),
            ));
        }

        // Step 3-13 Create CSpace and add all caps that the PD code and libmicrokit need to access.
        let pd_cnode_obj_id = capdl_util_make_cnode_obj(
            &mut spec_container,
            &pd.name,
            PD_CAP_BITS,
            caps_to_insert_to_pd_cspace,
        );
        let pd_guard_size = kernel_config.cap_address_bits - PD_CAP_BITS as u64;
        let pd_cnode_cap = capdl_util_make_cnode_cap(pd_cnode_obj_id, 0, pd_guard_size as u8);
        pd_shadow_cspace.get_mut(&pd_global_idx).unwrap()[CapMapType::Cnode as usize] =
            Some(pd_cnode_cap.clone());
        caps_to_bind_to_tcb.push(capdl_util_make_cte(
            TcbBoundSlot::CSpace as u32,
            pd_cnode_cap,
        ));
        pd_id_to_cspace_id.insert(pd_global_idx, pd_cnode_obj_id);

        // Step 3-14 Set the TCB parameters and all the various caps that we need to bind to this TCB.
        if let Object::Tcb(pd_tcb) = &mut spec_container
            .get_root_object_mut(pd_tcb_obj_id)
            .unwrap()
            .object
        {
            pd_tcb.extra.sp = Word(kernel_config.pd_stack_top());
            pd_tcb.extra.master_fault_ep = None; // Not used on MCS kernel.
            pd_tcb.extra.prio = pd.priority;
            pd_tcb.extra.max_prio = pd.priority;
            pd_tcb.extra.resume = true;

            pd_tcb.slots.extend(caps_to_bind_to_tcb);
            // Stylistic purposes only
            pd_tcb.slots.sort_by_key(|cte| usize::from(cte.slot));
        } else {
            unreachable!("internal bug: build_capdl_spec() got a non TCB object ID when trying to set TCB parameters for the monitor.");
        }

        // Step 3-15 bind this PD's TCB to the monitor, this accomplish two purposes:
        // 1. Allow PDs' TCBs to be named to their proper name in SDF in debug config.
        // 2. Allow passive PDs.
        capdl_util_insert_cap_into_cspace(
            &mut spec_container,
            mon_cnode_obj_id,
            (MON_BASE_PD_TCB_CAP as usize + pd_global_idx) as u32,
            capdl_util_make_tcb_cap(pd_tcb_obj_id),
        );
        if pd.passive {
            // When a PD is passive, it will signal the Monitor once init() returns. The monitor will
            // then unbind the PD's TCB from its Scheduling Context and bind it to its Notification.
            capdl_util_insert_cap_into_cspace(
                &mut spec_container,
                mon_cnode_obj_id,
                (MON_BASE_SCHED_CONTEXT_CAP as usize + pd_global_idx) as u32,
                capdl_util_make_sc_cap(pd_sc_obj_id),
            );
            capdl_util_insert_cap_into_cspace(
                &mut spec_container,
                mon_cnode_obj_id,
                (MON_BASE_NOTIFICATION_CAP as usize + pd_global_idx) as u32,
                capdl_util_make_ntfn_cap(pd_ntfn_obj_id, true, true, 0),
            );
        }
    }

    // *********************************
    // Step 4. Create channels
    // *********************************
    for channel in system.channels.iter() {
        let pd_a_cspace_id = *pd_id_to_cspace_id.get(&channel.end_a.pd).unwrap();
        let pd_b_cspace_id = *pd_id_to_cspace_id.get(&channel.end_b.pd).unwrap();
        let pd_a_ntfn_id = *pd_id_to_ntfn_id.get(&channel.end_a.pd).unwrap();
        let pd_b_ntfn_id = *pd_id_to_ntfn_id.get(&channel.end_b.pd).unwrap();

        // We trust that the SDF parsing code have checked for duplicate IDs.
        if channel.end_a.notify {
            let pd_a_ntfn_cap_idx = PD_BASE_OUTPUT_NOTIFICATION_CAP + channel.end_a.id;
            let pd_a_ntfn_badge = 1 << channel.end_b.id;
            let pd_a_ntfn_cap = capdl_util_make_ntfn_cap(pd_b_ntfn_id, true, true, pd_a_ntfn_badge);
            capdl_util_insert_cap_into_cspace(
                &mut spec_container,
                pd_a_cspace_id,
                pd_a_ntfn_cap_idx as u32,
                pd_a_ntfn_cap,
            );
        }

        if channel.end_b.notify {
            let pd_b_ntfn_cap_idx = PD_BASE_OUTPUT_NOTIFICATION_CAP + channel.end_b.id;
            let pd_b_ntfn_badge = 1 << channel.end_a.id;
            let pd_b_ntfn_cap = capdl_util_make_ntfn_cap(pd_a_ntfn_id, true, true, pd_b_ntfn_badge);
            capdl_util_insert_cap_into_cspace(
                &mut spec_container,
                pd_b_cspace_id,
                pd_b_ntfn_cap_idx as u32,
                pd_b_ntfn_cap,
            );
        }

        if channel.end_a.pp {
            let pd_a_ep_cap_idx = PD_BASE_OUTPUT_ENDPOINT_CAP + channel.end_a.id;
            let pd_a_ep_badge = PPC_BADGE | channel.end_b.id;
            let pd_b_ep_id = *pd_id_to_ep_id.get(&channel.end_b.pd).unwrap();
            let pd_a_ep_cap =
                capdl_util_make_endpoint_cap(pd_b_ep_id, true, true, true, pd_a_ep_badge);
            capdl_util_insert_cap_into_cspace(
                &mut spec_container,
                pd_a_cspace_id,
                pd_a_ep_cap_idx as u32,
                pd_a_ep_cap,
            );
        }

        if channel.end_b.pp {
            let pd_b_ep_cap_idx = PD_BASE_OUTPUT_ENDPOINT_CAP + channel.end_b.id;
            let pd_b_ep_badge = PPC_BADGE | channel.end_a.id;
            let pd_a_ep_id = *pd_id_to_ep_id.get(&channel.end_a.pd).unwrap();
            let pd_b_ep_cap =
                capdl_util_make_endpoint_cap(pd_a_ep_id, true, true, true, pd_b_ep_badge);
            capdl_util_insert_cap_into_cspace(
                &mut spec_container,
                pd_b_cspace_id,
                pd_b_ep_cap_idx as u32,
                pd_b_ep_cap,
            );
        }
    }

    // *********************************
    // Step 5. Handle extra cap mappings
    // *********************************

    for (pd_dest_idx, pd) in system.protection_domains.iter().enumerate() {
        let pd_dest_cspace_id = *pd_id_to_cspace_id.get(&pd_dest_idx).unwrap();

        for cap_map in pd.cap_maps.iter() {
            let pd_src_idx = pd_name_to_id.get(&cap_map.pd_name).ok_or(format!(
                "PD: '{}', does not exist when trying to map extra TCB cap into PD: '{}'",
                cap_map.pd_name, pd.name
            ))?;

            let pd_obj = pd_shadow_cspace.get(pd_src_idx).unwrap()[cap_map.cap_type as usize]
                .as_ref()
                .unwrap();
            // Map this into the destination pd's cspace and the specified slot.
            capdl_util_insert_cap_into_cspace(
                &mut spec_container,
                pd_dest_cspace_id,
                (PD_BASE_USER_CAPS + cap_map.dest_cspace_slot) as u32,
                pd_obj.clone(),
            );
        }
    }

    // *********************************
    // Step 6. Sort the root objects
    // *********************************
    // The CapDL initialiser expects objects with paddr to come first, then sorted by size so that the
    // allocation algorithm at run-time can run more efficiently.
    // Capabilities to objects in CapDL are referenced by the object's index in the root objects
    // vector. Since sorting the objects will shuffle them, we need to:
    // 1. Record all root objects name + original index.
    // 2. Sort paddr first, size bits descending and break tie alphabetically.
    // 3. Record all of the root objects new index.
    // 4. Recurse through every cap, for any cap bearing the original object ID, write the new object ID.

    // Step 6-1
    let mut obj_name_to_old_id: HashMap<String, ObjectId> = HashMap::new();
    for (id, obj) in spec_container.spec.objects.iter().enumerate() {
        obj_name_to_old_id.insert(obj.name.as_ref().unwrap().clone(), id.into());
    }

    // Step 6-2
    spec_container.spec.objects.sort_by(|a, b| {
        // Objects with paddrs always come first.
        if a.object.paddr().is_none() && b.object.paddr().is_some() {
            return Ordering::Greater;
        } else if a.object.paddr().is_some() && b.object.paddr().is_none() {
            return Ordering::Less;
        }

        // If both have paddrs, make the lower paddr come first.
        if a.object.paddr().is_some() && b.object.paddr().is_some() {
            let a_paddr = u64::from(a.object.paddr().unwrap());
            let b_paddr = u64::from(b.object.paddr().unwrap());
            let phys_addr_order = a_paddr.cmp(&b_paddr);
            if phys_addr_order != Ordering::Equal {
                return phys_addr_order;
            }
        }

        // Both have no paddr or equal paddr, break tie by object size (descending) and name.
        let a_size_bit = capdl_obj_physical_size_bits(&a.object, kernel_config);
        let b_size_bit = capdl_obj_physical_size_bits(&b.object, kernel_config);

        let size_cmp = a_size_bit.cmp(&b_size_bit).reverse();
        if size_cmp == Ordering::Equal {
            let name_cmp = a.name.cmp(&b.name);
            if name_cmp == Ordering::Equal {
                // Make sure the sorting function implement a total order to comply with .sort_by()'s doc.
                unreachable!(
                    "internal bug: object names must be unique! {}",
                    a.name.as_ref().unwrap()
                );
            }
            name_cmp
        } else {
            size_cmp
        }
    });

    // Step 6-3
    let mut obj_old_id_to_new_id: HashMap<ObjectId, ObjectId> = HashMap::new();
    for (new_id, obj) in spec_container.spec.objects.iter().enumerate() {
        obj_old_id_to_new_id.insert(
            *obj_name_to_old_id.get(obj.name.as_ref().unwrap()).unwrap(),
            new_id.into(),
        );
    }

    // Step 6-4
    for obj in spec_container.spec.objects.iter_mut() {
        match obj.object.slots_mut() {
            Some(caps) => {
                for cte in caps {
                    let old_id = cte.cap.obj();
                    let new_id = obj_old_id_to_new_id.get(&old_id).unwrap();
                    cte.cap.set_obj(*new_id);
                }
            }
            None => continue,
        }
    }
    for irq in spec_container.spec.irqs.iter_mut() {
        irq.handler = *obj_old_id_to_new_id.get(&irq.handler).unwrap();
    }

    // Only for aesthetic purposes:
    // Sort cap entries by their index.
    spec_container
        .spec
        .irqs
        .sort_by_key(|irq_entry| u64::from(irq_entry.irq));
    spec_container
        .spec
        .objects
        .iter_mut()
        .filter(|named_object| matches!(named_object.object, Object::CNode(_)))
        .for_each(|cnode_named_obj: &mut CapDLNamedObject| {
            cnode_named_obj
                .object
                .slots_mut()
                .unwrap()
                .sort_by_key(|cte| cte.slot.0)
        });

    Ok(spec_container)
}
