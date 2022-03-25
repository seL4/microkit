import capdl

from typing import (Any, NamedTuple, Optional, Tuple, Union)

from capdl.Object import (register_object_sizes, ObjectType)

from sel4coreplat.util import (MemoryRegion)
from sel4coreplat.sysxml import (ProtectionDomain)
from sel4coreplat.sysxml import (SysMap, SysMemoryRegion)

aarch64_sizes = {
    ObjectType.seL4_TCBObject: 11,
    ObjectType.seL4_EndpointObject: 4,
    ObjectType.seL4_NotificationObject: 6,
    ObjectType.seL4_SmallPageObject: 12,
    ObjectType.seL4_LargePageObject: 21,
    ObjectType.seL4_ASID_Pool: 12 ,
    ObjectType.seL4_ASID_Table: 9,
    ObjectType.seL4_Slot: 5,
    # ObjectType.seL4_Value_MinUntypedBits: 4,
    # ObjectType.seL4_Value_MaxUntypedBits: 47,
    # ObjectType.seL4_Value_BadgeBits: 64,
    ObjectType.seL4_RTReplyObject: 5,
    ObjectType.seL4_VCPU: 12,
    ObjectType.seL4_PageTableObject: 12,
    ObjectType.seL4_PageDirectoryObject: 12,
    ObjectType.seL4_ARM_SectionObject: 21,
    ObjectType.seL4_ARM_SuperSectionObject: 25,
    ObjectType.seL4_HugePageObject: 30,
    ObjectType.seL4_AARCH64_PGD: 12,
    ObjectType.seL4_AARCH64_PUD: 12,
    ObjectType.seL4_IOPageTableObject: 12,
    ObjectType.seL4_X64_PDPT: 12,
    ObjectType.seL4_X64_PML4: 12,
    ObjectType.seL4_SchedContextObject: 8,
    # ObjectType.seL4_IOPorts: 0,
    # ObjectType.seL4_IODevice: 0,
    # ObjectType.seL4_ARMIODevice: 0,
    # ObjectType.seL4_IRQ: 0,
    # ObjectType.seL4_IOAPICIRQ: 0,
    # ObjectType.seL4_MSIIRQ: 0,
    # ObjectType.seL4_ARMIRQ: 0,
    # ObjectType.seL4_ARMSID: 0,
    # ObjectType.seL4_ARMCB: 0,
}

def register_aarch64_sizes() -> None:
    register_object_sizes(aarch64_sizes)

def cdlsafe(string: str) -> str:
    return ''.join(c.lower() if c.isalnum() else '_' for c in string)

MRInfoNotELF = NamedTuple('MRInfoNotELF', [])
MRInfoELF = NamedTuple('MRInfoELF', [('pd_name', str), ('seg_index', int)])
MRInfo = Union[MRInfoNotELF, MRInfoELF]

PageInfoNotELF = NamedTuple('PageInfoNotELF', [])
PageInfoELF = NamedTuple('PageInfoELF', [('pd_name', str), ('seg_index', int), ('index', int)])
PageInfo = Union[PageInfoNotELF, PageInfoELF]

def mrinfo_to_pageinfo(mri: MRInfo, index: int) -> PageInfo:
    if isinstance(mri, MRInfoELF):
        return PageInfoELF(mri.pd_name, mri.seg_index, index)
    return PageInfoNotELF()

GlobalDir = NamedTuple('GlobalDir', [('pd_index', int)])
UpperDir = NamedTuple('UpperDir', [('pd_index',int)])
LowerDir = NamedTuple('LowerDir', [('pd_index',int)])
PTable = NamedTuple('PTable', [('pd_index',int)])
PFrame = NamedTuple('PFrame', [('pd_index',int),('page_size', int)])
MTESort = Union[GlobalDir, UpperDir, LowerDir, PTable, PFrame]
MTE = NamedTuple('MTE', # mapping table entry
                [('sort', MTESort),
                 ('vaddr', int)])

alignment_of_sort = {
    PFrame: 12,
    PTable: 12 + 9,
    LowerDir: 12 + 9 + 9,
    UpperDir: 12 + 9 + 9 + 9,
    GlobalDir: 12 + 9 + 9 + 9   # faux, see vaddr_to_gd
}

def alignment_of_mte(mte: MTE) -> int:
    if isinstance(mte.sort, PFrame):
        return alignment_of_sort[PFrame]
    if isinstance(mte.sort, PTable):
        return alignment_of_sort[PTable]
    if isinstance(mte.sort, LowerDir):
        return alignment_of_sort[LowerDir]
    if isinstance(mte.sort, UpperDir):
        return alignment_of_sort[UpperDir]
    if isinstance(mte.sort, GlobalDir):
        return alignment_of_sort[GlobalDir]
    raise ValueError()

def vaddr_to_gd(vaddr: int) -> MTE:
    # this is a faux MTE to facilitate parent lookups for PUDs
    alignment = alignment_of_sort[GlobalDir]
    truncated = (vaddr >> alignment) << alignment
    return MTE(GlobalDir(0), truncated)

def vaddr_to_ud(vaddr: int) -> MTE:
    alignment = alignment_of_sort[UpperDir]
    truncated = (vaddr >> alignment) << alignment
    # FIXME These numbers are ad-hoc and will fail with multiple PDs.
    # vaddr fns should take an arg for pd_index instead
    return MTE(UpperDir(1), truncated)

def vaddr_to_d(vaddr: int) -> MTE:
    alignment = alignment_of_sort[LowerDir]
    truncated = (vaddr >> alignment) << alignment
    return MTE(LowerDir(2), truncated)

def vaddr_to_pt(vaddr: int) -> MTE:
    alignment = alignment_of_sort[PTable]
    truncated = (vaddr >> alignment) << alignment
    return MTE(PTable(3), truncated)

def vaddr_to_pf(vaddr: int, page_size: int) -> MTE:
    return MTE(PFrame(4, page_size), vaddr)

def parent_mte_of(mte: MTE) -> MTE:
    if isinstance(mte.sort, UpperDir):
        return vaddr_to_gd(mte.vaddr)
    if isinstance(mte.sort, LowerDir):
        return vaddr_to_ud(mte.vaddr)
    if isinstance(mte.sort, PTable):
        return vaddr_to_d(mte.vaddr)
    if isinstance(mte.sort, PFrame):
        return vaddr_to_pt(mte.vaddr)
    raise ValueError()

def mapping_slot_of(current_mte: MTE) -> int:
    parent_mte = parent_mte_of(current_mte)
    slot = current_mte.vaddr - parent_mte.vaddr
    alignment = alignment_of_mte(current_mte)
    return (slot >> alignment)




#       root_cnode_cap,                                # Z: destination CNode
#       0,                                             # Z: destination index
#       root_cnode_bits,                               # Z: destination depth
#       INIT_CNODE_CAP_ADDRESS,                        # Z: source CNode
#       INIT_CNODE_CAP_ADDRESS,                        # Z: source CSlot index
#       kernel_config.cap_address_bits,                # Z: source depth
#       SEL4_RIGHTS_ALL,                               # Z: rights inherited by minted cap
#       guard                                          # Z: badge


# client_client_0_control_tcb = tcb (
#   addr: 0x53e000,
#   ip: 0x405fe8,
#   sp: 0x53c000,
#   prio: 254,
#   max_prio: 254,
#   affinity: 0,
#   init: [1],
#   fault_ep: 0x00000002)
