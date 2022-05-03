# Copyright 2022, University of New South Wales, Sydney
#
# SPDX-License-Identifier: BSD-2-Clause
#

import capdl

from typing import (Any, NamedTuple, Optional, Tuple, Union)

from capdl.Object import (register_object_sizes, ObjectType)

from sel4coreplat.util import (MemoryRegion)
from sel4coreplat.sysxml import (ProtectionDomain)
from sel4coreplat.sysxml import (SysMap, SysMemoryRegion)


def cdlsafe(string: str) -> str:
    '''Turns a string into an object name that can be safely used in CapDL specs.'''
    return ''.join(c.lower() if c.isalnum() else '_' for c in string)

aarch64_sizes = {
    "seL4_TCBObject": 11,
    "seL4_EndpointObject": 4,
    "seL4_NotificationObject": 6,
    "seL4_SmallPageObject": 12,
    "seL4_LargePageObject": 21,
    "seL4_ASID_Pool": 12 ,
    "seL4_ASID_Table": 9,
    "seL4_Slot": 5,
    # "seL4_Value_MinUntypedBits": 4,
    # "seL4_Value_MaxUntypedBits": 47,
    # "seL4_Value_BadgeBits": 64,
    "seL4_RTReplyObject": 5,
    "seL4_VCPU": 12,
    "seL4_PageTableObject": 12,
    "seL4_PageDirectoryObject": 12,
    "seL4_ARM_SectionObject": 21,
    "seL4_ARM_SuperSectionObject": 25,
    "seL4_HugePageObject": 30,
    "seL4_AARCH64_PGD": 12,
    "seL4_AARCH64_PUD": 12,
    "seL4_IOPageTableObject": 12,
    "seL4_X64_PDPT": 12,
    "seL4_X64_PML4": 12,
    "seL4_SchedContextObject": 8,
    # "seL4_IOPorts": 0,
    # "seL4_IODevice": 0,
    # "seL4_ARMIODevice": 0,
    # "seL4_IRQ": 0,
    # "seL4_IOAPICIRQ": 0,
    # "seL4_MSIIRQ": 0,
    # "seL4_ARMIRQ": 0,
    # "seL4_ARMSID": 0,
    # "seL4_ARMCB": 0,
}

def register_aarch64_sizes() -> None:
    register_object_sizes(aarch64_sizes)


# Structured objects for keeping track of CDL-specific info in __main__:

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

# Structured objects for keeping track of the mapping table.
# A mapping table is required because the capdl initialiser sort of reverses
# how sel4 expects things to be mapped. In sel4 you provide a vaddr that is
# resolved from the top of the vspace. Think of the vaddr as providing the
# path to get from the top-level paging structure (the pgd), all the way to
# the specific slot. While in capdl you construct this vaddr from its
# components (the slot being mapped to in the current level, and the slots
# that the various structures are mapped to in the higher levels).
# Hence, to generate the CapDL, we need the mapping table to reverse this
# mapping.

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
    '''Returns the slot number of an MTE inside its parent in the CapDL.'''
    parent_mte = parent_mte_of(current_mte)
    slot = current_mte.vaddr - parent_mte.vaddr
    alignment = alignment_of_mte(current_mte)
    return (slot >> alignment)

