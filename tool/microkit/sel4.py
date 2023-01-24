#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
from dataclasses import dataclass, fields
from enum import IntEnum
from typing import List, Optional, Set, Tuple
from struct import pack, Struct

from microkit.util import MemoryRegion, DisjointMemoryRegion, UserError, lsb, round_down, round_up
from microkit.elf import ElfFile


SLOT_BITS = 5
SLOT_SIZE = 1 << SLOT_BITS

SEL4_TCB_SIZE = (1 << 11)
SEL4_ENDPOINT_SIZE = (1 << 4)
SEL4_NOTIFICATION_SIZE = (1 << 6)
SEL4_REPLY_SIZE = (1 << 5)
SEL4_PAGE_TABLE_SIZE = (1 << 12)
SEL4_PAGE_DIRECTORY_SIZE = (1 << 12)
SEL4_PAGE_UPPER_DIRECTORY_SIZE = (1 << 12)
SEL4_LARGE_PAGE_SIZE = (2 * 1024 * 1024)
SEL4_SMALL_PAGE_SIZE = (4 * 1024)
SEL4_VSPACE_SIZE = (4 * 1024)
SEL4_ASID_POOL_SIZE = (1 << 12)


# Kernel Objects:

SEL4_UNTYPED_OBJECT = 0

SEL4_TCB_OBJECT = 1
SEL4_ENDPOINT_OBJECT = 2
SEL4_NOTIFICATION_OBJECT = 3
SEL4_CNODE_OBJECT = 4
SEL4_SCHEDCONTEXT_OBJECT = 5
SEL4_REPLY_OBJECT = 6

SEL4_HUGE_PAGE_OBJECT = 7
SEL4_PAGE_UPPER_DIRECTORY_OBJECT = 8
SEL4_PAGE_GLOBAL_DIRECTORY_OBJECT = 9
SEL4_SMALL_PAGE_OBJECT = 10
SEL4_LARGE_PAGE_OBJECT = 11
SEL4_PAGE_TABLE_OBJECT = 12
SEL4_PAGE_DIRECTORY_OBJECT = 13

SEL4_VSPACE_OBJECT = SEL4_PAGE_GLOBAL_DIRECTORY_OBJECT

SEL4_OBJECT_TYPE_NAMES = {
    SEL4_UNTYPED_OBJECT: "SEL4_UNTYPED_OBJECT",
    SEL4_TCB_OBJECT: "SEL4_TCB_OBJECT",
    SEL4_ENDPOINT_OBJECT: "SEL4_ENDPOINT_OBJECT",
    SEL4_NOTIFICATION_OBJECT: "SEL4_NOTIFICATION_OBJECT",
    SEL4_CNODE_OBJECT: "SEL4_CNODE_OBJECT",
    SEL4_SCHEDCONTEXT_OBJECT: "SEL4_SCHEDCONTEXT_OBJECT",
    SEL4_REPLY_OBJECT: "SEL4_REPLY_OBJECT",
    SEL4_HUGE_PAGE_OBJECT: "SEL4_HUGE_PAGE_OBJECT",
    SEL4_PAGE_UPPER_DIRECTORY_OBJECT: "SEL4_PAGE_UPPER_DIRECTORY_OBJECT",
    SEL4_PAGE_GLOBAL_DIRECTORY_OBJECT: "SEL4_PAGE_GLOBAL_DIRECTORY_OBJECT",
    SEL4_SMALL_PAGE_OBJECT: "SEL4_SMALL_PAGE_OBJECT",
    SEL4_LARGE_PAGE_OBJECT: "SEL4_LARGE_PAGE_OBJECT",
    SEL4_PAGE_TABLE_OBJECT: "SEL4_PAGE_TABLE_OBJECT",
    SEL4_PAGE_DIRECTORY_OBJECT: "SEL4_PAGE_DIRECTORY_OBJECT",
}

FIXED_OBJECT_SIZES = {
    SEL4_TCB_OBJECT: SEL4_TCB_SIZE,
    SEL4_ENDPOINT_OBJECT: SEL4_ENDPOINT_SIZE,
    SEL4_NOTIFICATION_OBJECT: SEL4_NOTIFICATION_SIZE,
    SEL4_REPLY_OBJECT: SEL4_REPLY_SIZE,

    SEL4_VSPACE_OBJECT: SEL4_VSPACE_SIZE,
    SEL4_PAGE_UPPER_DIRECTORY_OBJECT: SEL4_PAGE_UPPER_DIRECTORY_SIZE,
    SEL4_PAGE_DIRECTORY_OBJECT: SEL4_PAGE_DIRECTORY_SIZE,
    SEL4_PAGE_TABLE_OBJECT: SEL4_PAGE_TABLE_SIZE,

    SEL4_LARGE_PAGE_OBJECT: SEL4_LARGE_PAGE_SIZE,
    SEL4_SMALL_PAGE_OBJECT: SEL4_SMALL_PAGE_SIZE,
}

VARIABLE_SIZE_OBJECTS = {
    SEL4_CNODE_OBJECT,
    SEL4_UNTYPED_OBJECT,
    SEL4_SCHEDCONTEXT_OBJECT,
}

SEL4_RIGHTS_WRITE = 1
SEL4_RIGHTS_READ = 2
SEL4_RIGHTS_GRANT = 4
SEL4_RIGHTS_GRANT_REPLY = 8

SEL4_RIGHTS_ALL = 0xf

SEL4_ARM_PAGE_CACHEABLE = 1
SEL4_ARM_PARITY_ENABLED = 2
SEL4_ARM_EXECUTE_NEVER = 4

SEL4_ARM_DEFAULT_VMATTRIBUTES = 3

SEL4_ARM_CACHE_I = 1
SEL4_ARM_CACHE_D = 2
SEL4_ARM_CACHE_ID = 3

# FIXME: There should be a better way of determining these, so they don't
# have to be hard coded
INIT_NULL_CAP_ADDRESS = 0
INIT_TCB_CAP_ADDRESS = 1
INIT_CNODE_CAP_ADDRESS = 2
INIT_VSPACE_CAP_ADDRESS = 3
IRQ_CONTROL_CAP_ADDRESS = 4  # Singleton
ASID_CONTROL_CAP_ADDRESS = 5  # Singleton
INIT_ASID_POOL_CAP_ADDRESS = 6
IO_PORT_CONTROL_CAP_ADDRESS = 7  # Null on this platform
IO_SPACE_CAP_ADDRESS = 8  # Null on this platform
BOOT_INFO_FRAME_CAP_ADDRESS = 9
INIT_THREAD_IPC_BUFFER_CAP_ADDRESS = 10
DOMAIN_CAP_ADDRESS = 11
SMMU_SID_CONTROL_CAP_ADDRESS = 12
SMMU_CB_CONTROL_CAP_ADDRESS = 13
INIT_THREAD_SC_CAP_ADDRESS = 14


def _get_n_paging(region: MemoryRegion, bits: int) -> int:
    start = round_down(region.base, 1 << bits)
    end = round_up(region.end, 1 << bits)
    return (end - start) // (1 << bits)


def _get_arch_n_paging(region: MemoryRegion) -> int:
    PT_INDEX_OFFSET  =  12
    PD_INDEX_OFFSET  =  (PT_INDEX_OFFSET + 9)
    PUD_INDEX_OFFSET =  (PD_INDEX_OFFSET + 9)
    PGD_INDEX_OFFSET =  (PUD_INDEX_OFFSET + 9)

    return (
        _get_n_paging(region, PGD_INDEX_OFFSET) +
        _get_n_paging(region, PUD_INDEX_OFFSET) +
        _get_n_paging(region, PD_INDEX_OFFSET)
    )


def calculate_rootserver_size(initial_task_region: MemoryRegion) -> int:
    # FIXME: These constants should ideally come from the config / kernel
    # binary not be hard coded here.
    # But they are constant so it isn't too bad.
    # This is specifically for aarch64
    slot_bits = 5  # seL4_SlotBits
    root_cnode_bits = 12  # CONFIG_ROOT_CNODE_SIZE_BITS
    tcb_bits = 11  # seL4_TCBBits
    page_bits = 12  # seL4_PageBits
    asid_pool_bits = 12  # seL4_ASIDPoolBits
    vspace_bits = 12  #seL4_VSpaceBits
    page_table_bits = 12  # seL4_PageTableBits
    min_sched_context_bits = 8 # seL4_MinSchedContextBits

    size = 0
    size += 1 << (root_cnode_bits + slot_bits)
    size += 1 << (tcb_bits)
    size += 2 * (1 << page_bits)
    size += 1 << asid_pool_bits
    size += 1 << vspace_bits
    size += _get_arch_n_paging(initial_task_region) * (1 << page_table_bits)
    size += 1 <<min_sched_context_bits

    return size


class Sel4Aarch64Regs:
    """
typedef struct seL4_UserContext_ {
    /* frame registers */
    seL4_Word pc, sp, spsr, x0, x1, x2, x3, x4, x5, x6, x7, x8, x16, x17, x18, x29, x30;
    /* other integer registers */
    seL4_Word x9, x10, x11, x12, x13, x14, x15, x19, x20, x21, x22, x23, x24, x25, x26, x27, x28;
    /* Thread ID registers */
    seL4_Word tpidr_el0, tpidrro_el0;
} seL4_UserContext;

    """
    # FIXME: This is pretty terrible, but for now... explicit better than implicit
    # NOTE: We could optimize so that we can see how many register are actually set
    # in a given set to reduce space
    def __init__(self,
        pc: Optional[int] = None,
        sp: Optional[int] = None,
        spsr: Optional[int] = None,
        x0: Optional[int] = None,
        x1: Optional[int] = None,
        x2: Optional[int] = None,
        x3: Optional[int] = None,
        x4: Optional[int] = None,
        x5: Optional[int] = None,
        x6: Optional[int] = None,
        x7: Optional[int] = None,
        x8: Optional[int] = None,
        x16: Optional[int] = None,
        x17: Optional[int] = None,
        x18: Optional[int] = None,
        x29: Optional[int] = None,
        x30: Optional[int] = None,
        x9: Optional[int] = None,
        x10: Optional[int] = None,
        x11: Optional[int] = None,
        x12: Optional[int] = None,
        x13: Optional[int] = None,
        x14: Optional[int] = None,
        x15: Optional[int] = None,
        x19: Optional[int] = None,
        x20: Optional[int] = None,
        x21: Optional[int] = None,
        x22: Optional[int] = None,
        x23: Optional[int] = None,
        x24: Optional[int] = None,
        x25: Optional[int] = None,
        x26: Optional[int] = None,
        x27: Optional[int] = None,
        x28: Optional[int] = None,
        tpidr_el0: Optional[int] = None,
        tpidrro_el0: Optional[int] = None,
    ):
        self.pc          = pc
        self.sp          = sp
        self.spsr        = spsr
        self.x0          = x0
        self.x1          = x1
        self.x2          = x2
        self.x3          = x3
        self.x4          = x4
        self.x5          = x5
        self.x6          = x6
        self.x7          = x7
        self.x8          = x8
        self.x16         = x16
        self.x17         = x17
        self.x18         = x18
        self.x29         = x29
        self.x30         = x30
        self.x9          = x9
        self.x10         = x10
        self.x11         = x11
        self.x12         = x12
        self.x13         = x13
        self.x14         = x14
        self.x15         = x15
        self.x19         = x19
        self.x20         = x20
        self.x21         = x21
        self.x22         = x22
        self.x23         = x23
        self.x24         = x24
        self.x25         = x25
        self.x26         = x26
        self.x27         = x27
        self.x28         = x28
        self.tpidr_el0   = tpidr_el0
        self.tpidrro_el0 = tpidrro_el0

    def count(self) -> int:
        # FIXME: Optimize when most are none
        return len(self.as_tuple())

    def as_tuple(self) -> Tuple[int, ...]:
        raw =  (
        self.pc         ,
        self.sp         ,
        self.spsr       ,
        self.x0         ,
        self.x1         ,
        self.x2         ,
        self.x3         ,
        self.x4         ,
        self.x5         ,
        self.x6         ,
        self.x7         ,
        self.x8         ,
        self.x16        ,
        self.x17        ,
        self.x18        ,
        self.x29        ,
        self.x30        ,
        self.x9         ,
        self.x10        ,
        self.x11        ,
        self.x12        ,
        self.x13        ,
        self.x14        ,
        self.x15        ,
        self.x19        ,
        self.x20        ,
        self.x21        ,
        self.x22        ,
        self.x23        ,
        self.x24        ,
        self.x25        ,
        self.x26        ,
        self.x27        ,
        self.x28        ,
        self.tpidr_el0  ,
        self.tpidrro_el0,
        )
        return tuple(0 if x is None else x for x in raw)

    def __str__(self):
        return self.as_tuple().__str__()

    def __repr__(self):
        return self.as_tuple().__repr__()

class Sel4Label(IntEnum):
    # Untyped
    UntypedRetype = 1
    # TCB
    TCBReadRegisters = 2
    TCBWriteRegisters = 3
    TCBCopyRegisters = 4
    TCBConfigure = 5
    TCBSetPriority = 6
    TCBSetMCPriority = 7
    TCBSetSchedParams = 8
    TCBSetTimeoutEndpoint = 9
    TCBSetIPCBuffer = 10
    TCBSetSpace = 11
    TCBSuspend = 12
    TCBResume = 13
    TCBBindNotification = 14
    TCBUnbindNotification = 15
    TCBSetTLSBase = 16
    # CNode
    CNodeRevoke = 17
    CNodeDelete = 18
    CNodeCancelBadgedSends = 19
    CNodeCopy = 20
    CNodeMint = 21
    CNodeMove = 22
    CNodeMutate = 23
    CNodeRotate = 24
    # IRQ
    IRQIssueIRQHandler = 25
    IRQAckIRQ = 26
    IRQSetIRQHandler = 27
    IRQClearIRQHandler = 28
    # Domain
    DomainSetSet = 29
    # Sched
    SchedControlConfigureFlags = 30
    SchedContextBind = 31
    SchedContextUnbind = 32
    SchedContextUnbindObject = 33
    SchedContextConsume = 34
    SchedContextYieldTo = 35
    # ARM V Space
    ARMVSpaceClean_Data = 36
    ARMVSpaceInvalidate_Data = 37
    ARMVSpaceCleanInvalidate_Data = 38
    ARMVSpaceUnify_Instruction = 39
    # ARM Page Upper Directory
    ARMPageUpperDirectoryMap = 40
    ARMPageUpperDirectoryUnmap = 41
    ARMPageDirectoryMap = 42
    ARMPageDirectoryUnmap = 43
    # ARM Page table
    ARMPageTableMap = 44
    ARMPageTableUnmap = 45
    # ARM Page
    ARMPageMap = 46
    ARMPageUnmap = 47
    ARMPageClean_Data = 48
    ARMPageInvalidate_Data = 49
    ARMPageCleanInvalidate_Data = 50
    ARMPageUnify_Instruction = 51
    ARMPageGetAddress = 52
    # ARM Asid
    ARMASIDControlMakePool = 53
    ARMASIDPoolAssign = 54
    # ARM IRQ
    ARMIRQIssueIRQHandlerTrigger = 55


### Invocations

class Sel4Invocation:
    label: Sel4Label
    _extra_caps: Tuple[str, ...]
    _object_type: str
    _method_name: str

    def _generic_invocation(self, extra_caps: Tuple[int, ...], args: Tuple[int, ...]) -> bytes:
        repeat_count = self._repeat_count if hasattr(self, "_repeat_count") else None
        tag = self.message_info_new(self.label, 0, len(extra_caps), len(args))
        if repeat_count:
            tag |= ((repeat_count - 1) << 32)
        fmt = "<QQ" + ("Q" * (0 + len(extra_caps) + len(args)))
        all_args = (tag, self._service) + extra_caps + args
        base = pack(fmt, *all_args)
        if repeat_count:
            repeat_incr = self._repeat_incr
            extra_fmt = "<Q" + ("Q" * (0 + len(extra_caps) + len(args)))
            service: int = repeat_incr.get(fields(self)[0].name, 0)
            cap_args: Tuple[int, ...] = tuple(repeat_incr.get(f.name, 0) for f in fields(self)[1:] if f.name in self._extra_caps)
            val_args: Tuple[int, ...] = tuple(repeat_incr.get(f.name, 0) for f in fields(self)[1:] if f.name not in self._extra_caps)
            extra = pack(extra_fmt, *((service, ) + cap_args + val_args))
        else:
            extra = b''
        return base + extra

    @property
    def _service(self) -> int:
        v = getattr(self, fields(self)[0].name)
        assert isinstance(v, int)
        return v

    @property
    def _args(self) -> List[Tuple[str, int]]:
        arg_names = [f.name for f in fields(self)[1:]]
        return [(nm, getattr(self, nm)) for nm in arg_names]

    @staticmethod
    def message_info_new(label: Sel4Label, caps: int, extra_caps: int, length: int) -> int:
        assert label < (1 << 50)
        assert caps < 8
        assert extra_caps < 4
        assert length < 0x80
        return label << 12 | caps << 9 | extra_caps << 7 | length

    def _get_raw_invocation(self) -> bytes:
        cap_args = tuple(val for nm, val in self._args if nm in self._extra_caps)
        val_args = tuple(val for nm, val in self._args if nm not in self._extra_caps)
        return self._generic_invocation(cap_args, val_args)

    def repeat(self, count: int, **kwargs: int) -> None:
        if count > 1:
            field_names: Set[str] = {f.name for f in fields(self)}
            assert len(kwargs) > 0
            for nm in kwargs:
                assert nm in field_names
            self._repeat_count = count
            self._repeat_incr = kwargs


@dataclass
class Sel4UntypedRetype(Sel4Invocation):
    _object_type = "Untyped"
    _method_name = "Retype"
    _extra_caps = ("root", )
    label = Sel4Label.UntypedRetype
    untyped: int
    object_type: int
    size_bits: int
    root: int
    node_index: int
    node_depth: int
    node_offset: int
    num_objects: int


@dataclass
class Sel4TcbSetSchedParams(Sel4Invocation):
    _object_type = "TCB"
    _method_name = "SetSchedParams"
    _extra_caps = ("authority", "sched_context", "fault_ep")
    label = Sel4Label.TCBSetSchedParams
    tcb: int
    authority: int
    mcp: int
    priority: int
    sched_context: int
    fault_ep: int


@dataclass
class Sel4TcbSetSpace(Sel4Invocation):
    _object_type = "TCB"
    _method_name = "SetSpace"
    _extra_caps = ("fault_ep", "cspace_root", "vspace_root")
    label = Sel4Label.TCBSetSpace
    tcb: int
    fault_ep: int
    cspace_root: int
    cspace_root_data: int
    vspace_root: int
    vspace_root_data: int


@dataclass
class Sel4TcbSetIpcBuffer(Sel4Invocation):
    _object_type = "TCB"
    _method_name = "SetIPCBuffer"
    _extra_caps = ("buffer_frame", )
    label = Sel4Label.TCBSetIPCBuffer
    tcb: int
    buffer: int
    buffer_frame: int


@dataclass
class Sel4TcbResume(Sel4Invocation):
    _object_type = "TCB"
    _method_name = "Resume"
    _extra_caps = ()
    label = Sel4Label.TCBResume
    tcb: int


@dataclass
class Sel4TcbWriteRegisters(Sel4Invocation):
    _object_type = "TCB"
    _method_name = "WriteRegisters"
    _extra_caps = ()
    label = Sel4Label.TCBWriteRegisters
    tcb: int
    resume: bool
    arch_flags: int
    regs: Sel4Aarch64Regs

    def _get_raw_invocation(self) -> bytes:
        params = (
            self.arch_flags << 8 | 1 if self.resume else 0,
            self.regs.count()
        ) + self.regs.as_tuple()

        return self._generic_invocation((), params)

@dataclass
class Sel4TcbBindNotification(Sel4Invocation):
    _object_type = "TCB"
    _method_name = "BindNotification"
    _extra_caps = ("notification", )
    label = Sel4Label.TCBBindNotification
    tcb: int
    notification: int


@dataclass
class Sel4AsidPoolAssign(Sel4Invocation):
    _object_type = "ASID Pool"
    _method_name = "Assign"
    _extra_caps = ("vspace", )
    label = Sel4Label.ARMASIDPoolAssign
    asid_pool: int
    vspace: int


@dataclass
class Sel4IrqControlGet(Sel4Invocation):
    _object_type = "IRQ Control"
    _method_name = "Get"
    _extra_caps = ("dest_root", )
    label = Sel4Label.IRQIssueIRQHandler
    irq_control: int
    irq: int
    dest_root: int
    dest_index: int
    dest_depth: int


@dataclass
class Sel4IrqHandlerSetNotification(Sel4Invocation):
    _object_type = "IRQ Handler"
    _method_name = "SetNotification"
    _extra_caps = ("notification", )
    label = Sel4Label.IRQSetIRQHandler
    irq_handler: int
    notification: int


@dataclass
class Sel4PageUpperDirectoryMap(Sel4Invocation):
    _object_type = "Page Upper Directory"
    _method_name = "Map"
    _extra_caps = ("vspace", )
    label = Sel4Label.ARMPageUpperDirectoryMap
    page_upper_directory: int
    vspace: int
    vaddr: int
    attr: int


@dataclass
class Sel4PageDirectoryMap(Sel4Invocation):
    _object_type = "Page Directory"
    _method_name = "Map"
    _extra_caps = ("vspace", )
    label = Sel4Label.ARMPageDirectoryMap
    page_directory: int
    vspace: int
    vaddr: int
    attr: int


@dataclass
class Sel4PageTableMap(Sel4Invocation):
    _object_type = "Page Table"
    _method_name = "Map"
    _extra_caps = ("vspace", )
    label = Sel4Label.ARMPageTableMap
    page_table: int
    vspace: int
    vaddr: int
    attr: int


@dataclass
class Sel4PageMap(Sel4Invocation):
    _object_type = "Page"
    _method_name = "Map"
    _extra_caps = ("vspace", )
    label = Sel4Label.ARMPageMap
    page: int
    vspace: int
    vaddr: int
    rights: int
    attr: int


@dataclass
class Sel4CnodeMint(Sel4Invocation):
    _object_type = "CNode"
    _method_name = "Mint"
    _extra_caps = ("src_root", )
    label = Sel4Label.CNodeMint
    cnode: int
    dest_index: int
    dest_depth: int
    src_root: int
    src_obj: int
    src_depth: int
    rights: int
    badge: int

@dataclass
class Sel4CnodeCopy(Sel4Invocation):
    _object_type = "CNode"
    _method_name = "Copy"
    _extra_caps = ("src_root", )
    label = Sel4Label.CNodeCopy
    cnode: int
    dest_index: int
    dest_depth: int
    src_root: int
    src_obj: int
    src_depth: int
    rights: int

@dataclass
class Sel4CnodeMutate(Sel4Invocation):
    _object_type = "CNode"
    _method_name = "Mutate"
    _extra_caps = ("src_root", )
    label = Sel4Label.CNodeMutate
    cnode: int
    dest_index: int
    dest_depth: int
    src_root: int
    src_obj: int
    src_depth: int
    badge: int

@dataclass
class Sel4SchedControlConfigureFlags(Sel4Invocation):
    _object_type = "SchedControl"
    _method_name = "ConfigureFlags"
    _extra_caps = ("schedcontext", )
    label = Sel4Label.SchedControlConfigureFlags
    schedcontrol: int
    schedcontext: int
    budget: int
    period: int
    extra_refills: int
    badge: int
    flags: int

@dataclass(frozen=True, eq=True)
class UntypedObject:
    cap: int
    region: MemoryRegion
    is_device: bool

    @property
    def base(self) -> int:
        return self.region.base

    @property
    def size_bits(self) -> int:
        return lsb(self.region.end - self.region.base)



@dataclass(frozen=True, eq=True)
class KernelBootInfo:
    fixed_cap_count: int
    schedcontrol_cap: int
    paging_cap_count: int
    page_cap_count: int
    untyped_objects: List[UntypedObject]
    first_available_cap: int


@dataclass(frozen=True, eq=True)
class KernelConfig:
    word_size: int
    minimum_page_size: int
    paddr_user_device_top: int
    kernel_frame_size: int
    init_cnode_bits: int
    cap_address_bits: int
    fan_out_limit: int


@dataclass
class _KernelPartialBootInfo:
    device_memory: DisjointMemoryRegion
    normal_memory: DisjointMemoryRegion
    boot_region: MemoryRegion


def _kernel_device_addrs(kernel_elf: ElfFile) -> List[int]:
    """Extra the physical address of all kernel (only) devices"""
    kernel_devices = []
    kernel_frame_t = Struct("<QQII")
    vaddr, size = kernel_elf.find_symbol("kernel_device_frames")
    p_regs = kernel_elf.get_data(vaddr, size)
    offset = 0
    while offset < size:
        paddr, pptr, xn, ua = kernel_frame_t.unpack_from(p_regs, offset)
        if not ua:
            kernel_devices.append(paddr)
        offset += kernel_frame_t.size

    return kernel_devices


def _kernel_phys_mem(kernel_elf: ElfFile) -> List[Tuple[int, int]]:
    """Extract a list of normal memory from the kernel elf file."""
    phys_mem = []
    p_region_t = Struct("<QQ")
    vaddr, size = kernel_elf.find_symbol("avail_p_regs")
    p_regs = kernel_elf.get_data(vaddr, size)
    offset = 0
    while offset < size:
        start, end = p_region_t.unpack_from(p_regs, offset)
        phys_mem.append((start, end))
        offset += p_region_t.size

    return phys_mem


def _kernel_self_mem(kernel_elf: ElfFile) -> Tuple[int, int]:
    """Return the physical memory range used by the kernel itself."""
    base = kernel_elf.segments[0].phys_addr
    ki_end_v, _= kernel_elf.find_symbol("ki_end")
    ki_end_p = ki_end_v - kernel_elf.segments[0].virt_addr + base
    return (base, ki_end_p)


def _kernel_boot_mem(kernel_elf: ElfFile) -> MemoryRegion:
    base = kernel_elf.segments[0].phys_addr
    ki_boot_end_v, _ = kernel_elf.find_symbol("ki_boot_end")
    ki_boot_end_p = ki_boot_end_v - kernel_elf.segments[0].virt_addr + base
    return MemoryRegion(base, ki_boot_end_p)


def _rootserver_max_size_bits() -> int:
    slot_bits = 5  # seL4_SlotBits
    root_cnode_bits = 12  # CONFIG_ROOT_CNODE_SIZE_BITS
    vspace_bits = 12  #seL4_VSpaceBits

    cnode_size_bits = root_cnode_bits + slot_bits
    return max(cnode_size_bits, vspace_bits)


def _kernel_partial_boot(
        kernel_config: KernelConfig,
        kernel_elf: ElfFile) -> _KernelPartialBootInfo:
    """Emulate what happens during a kernel boot, up to the point
    where the reserved region is allocated.

    This factors the common parts of 'emulate_kernel_boot' and
    'emulate_kernel_boot_partial' to avoid code duplication.
    """
    # Determine the untyped caps of the system
    # This lets allocations happen correctly.
    device_memory = DisjointMemoryRegion()
    normal_memory = DisjointMemoryRegion()

    # Start by allocating the entire physical address space
    # as device memory.
    device_memory.insert_region(0, kernel_config.paddr_user_device_top)

    # Next, remove all the kernel devices.
    # NOTE: There is an assumption each kernel device is one frame
    # in size only. It's possible this assumption could break in the
    # future.
    for paddr in _kernel_device_addrs(kernel_elf):
        device_memory.remove_region(paddr, paddr + kernel_config.kernel_frame_size)

    # Remove all the actual physical memory from the device regions
    # but add it all to the actual normal memory regions
    for start, end in _kernel_phys_mem(kernel_elf):
        device_memory.remove_region(start, end)
        normal_memory.insert_region(start, end)

    # Remove the kernel image itself
    normal_memory.remove_region(*_kernel_self_mem(kernel_elf))

    # but get the boot region, we'll add that back later
    # FIXME: Why calcaultae it now if we add it back later?
    boot_region = _kernel_boot_mem(kernel_elf)

    return _KernelPartialBootInfo(device_memory, normal_memory, boot_region)


def emulate_kernel_boot_partial(
        kernel_config: KernelConfig,
        kernel_elf: ElfFile,
    ) -> DisjointMemoryRegion:
    """Return the memory available after a 'partial' boot emulation.

    This allows the caller to allocation a reserved memory region at an
    appropriate location.
    """
    partial_info = _kernel_partial_boot(kernel_config, kernel_elf)
    return partial_info.normal_memory


def emulate_kernel_boot(
        kernel_config: KernelConfig,
        kernel_elf: ElfFile,
        initial_task_phys_region: MemoryRegion,
        initial_task_virt_region: MemoryRegion,
        reserved_region: MemoryRegion) -> KernelBootInfo:
    """Emulate what happens during a kernel boot, generating a
    representation of the BootInfo struct."""
    # And the the reserved region
    assert initial_task_phys_region.size == initial_task_virt_region.size
    partial_info = _kernel_partial_boot(kernel_config, kernel_elf)
    normal_memory = partial_info.normal_memory
    device_memory = partial_info.device_memory
    boot_region = partial_info.boot_region

    normal_memory.remove_region(initial_task_phys_region.base, initial_task_phys_region.end)
    normal_memory.remove_region(reserved_region.base, reserved_region.end)

    # Now, the tricky part! determine which memory is used for the initial task objects
    initial_objects_size = calculate_rootserver_size(initial_task_virt_region)
    initial_objects_align = _rootserver_max_size_bits()

    # Find an appropriate region of normal memory to allocate the objects
    # from; this follows the same algorithm used within the kernel boot code
    # (or at least we hope it does!)
    for region in reversed(normal_memory._regions):
        start = round_down(region.end - initial_objects_size, 1 << initial_objects_align)
        if start >= region.base:
            normal_memory.remove_region(start, start + initial_objects_size)
            break
    else:
        raise Exception("Couldn't find appropriate region for initial task kernel objects")

    fixed_cap_count = 0xf
    sched_control_cap_count = 1
    paging_cap_count = _get_arch_n_paging(initial_task_virt_region)
    page_cap_count = initial_task_virt_region.size // kernel_config.minimum_page_size
    first_untyped_cap = fixed_cap_count + paging_cap_count + sched_control_cap_count + page_cap_count
    schedcontrol_cap = fixed_cap_count + paging_cap_count

    device_regions = reserved_region.aligned_power_of_two_regions() + device_memory.aligned_power_of_two_regions()
    normal_regions = boot_region.aligned_power_of_two_regions() + normal_memory.aligned_power_of_two_regions()
    untyped_objects = []
    for cap, r in enumerate(device_regions, first_untyped_cap):
        untyped_objects.append(UntypedObject(cap, r, True))
    for cap, r in enumerate(normal_regions, cap + 1):
        untyped_objects.append(UntypedObject(cap, r, False))

    return KernelBootInfo(
        fixed_cap_count = fixed_cap_count,
        paging_cap_count = paging_cap_count,
        page_cap_count = page_cap_count,
        schedcontrol_cap = schedcontrol_cap,
        first_available_cap = first_untyped_cap + len(device_regions) + len(normal_regions),
        untyped_objects = untyped_objects,
    )
