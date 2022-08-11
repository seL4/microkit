#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
"""
The purpose of this script is to take as input a system description XML file
and generate a system image suitable for loading by the platform bootloader.

The loader image in the current script is assumed to be a flat binary image
that is directly loaded into physical memory.

This makes use of the `altloader` (and alternative to the normal ELF loader
bootstrap). `altloader` initialises areas of physical memory as described by
a sequence a `regions`. It then jumps to the seL4 kernel.

The `altloader` passes two the kernel two regions of memory:

1: The initial task
2: A region of 'additional memory'.

TODO - Cleanup:

reporting:
  number of rebuilds required.
  warnings
  list all kernel objects
  name all kernel objects



The following abreviations are used in the source code:

* Capability => cap
* Address => addr
* Physical => phys

@ivanv be consistent about kernel_config vs config for KernelConfig usage,
  should really be called just Config or something, since it'll have more than
  just the kernel stuff in it

"""
import cProfile
import sys
import os
from argparse import ArgumentParser
from pathlib import Path
from dataclasses import dataclass
from struct import pack, Struct
from os import environ
from math import log2, ceil
from sys import argv, executable, stderr
from yaml import load  as yaml_load, Loader as YamlLoader

from typing import Dict, List, Optional, Tuple, Union

from sel4coreplat.elf import ElfFile
from sel4coreplat.util import kb, mb, lsb, msb, round_up, round_down, mask_bits, is_power_of_two, MemoryRegion, UserError
from sel4coreplat.sel4 import (
    Sel4Aarch64Regs,
    Sel4Invocation,
    Sel4AsidPoolAssign,
    Sel4PageUpperDirectoryMap,
    Sel4PageDirectoryMap,
    Sel4PageTableMap,
    Sel4PageMap,
    Sel4TcbSetSchedParams,
    Sel4TcbSetSpace,
    Sel4TcbSetIpcBuffer,
    Sel4TcbWriteRegisters,
    Sel4TcbBindNotification,
    Sel4TcbResume,
    Sel4CnodeMint,
    Sel4UntypedRetype,
    Sel4IrqControlGet,
    Sel4IrqHandlerSetNotification,
    Sel4SchedControlConfigureFlags,
    Sel4ArmVcpuSetTcb,
    emulate_kernel_boot,
    emulate_kernel_boot_partial,
    UntypedObject,
    KernelConfig,
    KernelBootInfo,
    FIXED_OBJECT_SIZES,
    SEL4_UNTYPED_OBJECT,
    SEL4_CNODE_OBJECT,
    SEL4_SCHEDCONTEXT_OBJECT,
    SEL4_TCB_OBJECT,
    SEL4_REPLY_OBJECT,
    SEL4_ENDPOINT_OBJECT,
    SEL4_NOTIFICATION_OBJECT,
    SEL4_VSPACE_OBJECT,
    SEL4_PAGE_UPPER_DIRECTORY_OBJECT,
    SEL4_PAGE_DIRECTORY_OBJECT,
    SEL4_SMALL_PAGE_OBJECT,
    SEL4_LARGE_PAGE_OBJECT,
    SEL4_PAGE_TABLE_OBJECT,
    SEL4_VCPU_OBJECT,
    SLOT_BITS,
    SLOT_SIZE,
    INIT_NULL_CAP_ADDRESS,
    INIT_TCB_CAP_ADDRESS,
    INIT_CNODE_CAP_ADDRESS,
    INIT_VSPACE_CAP_ADDRESS,
    INIT_ASID_POOL_CAP_ADDRESS,
    IRQ_CONTROL_CAP_ADDRESS,
    SEL4_RIGHTS_ALL,
    SEL4_RIGHTS_READ,
    SEL4_RIGHTS_WRITE,
    SEL4_ARM_DEFAULT_VMATTRIBUTES,
    SEL4_ARM_EXECUTE_NEVER,
    SEL4_ARM_PARITY_ENABLED,
    SEL4_ARM_PAGE_CACHEABLE,
    SEL4_LARGE_PAGE_SIZE,
    SEL4_PAGE_TABLE_SIZE,
    SEL4_OBJECT_TYPE_NAMES,
)
from sel4coreplat.sysxml import ProtectionDomain, xml2system, SystemDescription, PlatformDescription
from sel4coreplat.sysxml import SysMap, SysMemoryRegion # This shouldn't be needed here as such
from sel4coreplat.loader import Loader

# This is a workaround for: https://github.com/indygreg/PyOxidizer/issues/307
# Basically, pyoxidizer generates code that results in argv[0] being set to None.
# ArgumentParser() very much relies on a non-None argv[0]!
# This very simple work-around sets it to the package name.
if argv[0] is None:
    argv[0] = executable  # type: ignore


default_platform_description = PlatformDescription(
    page_sizes = (0x1_000, 0x200_000)
)

@dataclass
class MonitorConfig:
    untyped_info_symbol_name: str
    untyped_info_header_struct: Struct
    untyped_info_object_struct: Struct
    bootstrap_invocation_count_symbol_name: str
    bootstrap_invocation_data_symbol_name: str
    system_invocation_count_symbol_name: str

    def max_untyped_objects(self, symbol_size: int) -> int:
        return (symbol_size - self.untyped_info_header_struct.size) // self.untyped_info_object_struct.size

# The monitor config is fixed (unless the monitor C code
# changes the definitions of struct, or the name.
# While this is fixed, we dynamically determine the
# size actual data structures at run time where possible
# to allow for minor changes in the C code without requiring
# rework of this tool
MONITOR_CONFIG = MonitorConfig(
    untyped_info_symbol_name = "untyped_info",
    untyped_info_header_struct = Struct("<QQ"),
    untyped_info_object_struct = Struct("<QQQ"),
    bootstrap_invocation_count_symbol_name = "bootstrap_invocation_count",
    bootstrap_invocation_data_symbol_name = "bootstrap_invocation_data",
    system_invocation_count_symbol_name = "system_invocation_count",
)

# Will be either the notification or endpoint cap
INPUT_CAP_IDX = 1
FAULT_EP_CAP_IDX = 2
VSPACE_CAP_IDX = 3
REPLY_CAP_IDX = 4
BASE_OUTPUT_NOTIFICATION_CAP = 10
BASE_OUTPUT_ENDPOINT_CAP = BASE_OUTPUT_NOTIFICATION_CAP + 64
BASE_IRQ_CAP = BASE_OUTPUT_ENDPOINT_CAP + 64
BASE_TCB_CAP = BASE_IRQ_CAP + 64
BASE_VM_TCB_CAP = BASE_TCB_CAP + 64
MAX_SYSTEM_INVOCATION_SIZE = mb(128)
PD_CAPTABLE_BITS = 12
PD_CAP_SIZE = 256
PD_CAP_BITS = int(log2(PD_CAP_SIZE))
PD_SCHEDCONTEXT_SIZE = (1 << 8)


def mr_page_bytes(mr: SysMemoryRegion) -> int:
    return 0x1000 if mr.page_size is None else mr.page_size


@dataclass(frozen=True)
class KernelAllocation:
    untyped_cap_address: int  # Fixme: possibly this is an object, not an int?
    phys_addr: int
    allocation_order: int


@dataclass
class UntypedAllocator:
    untyped_object: UntypedObject
    allocation_point: int
    allocations: List[KernelAllocation]

    @property
    def base(self) -> int:
        return self.untyped_object.region.base

    @property
    def end(self) -> int:
        return self.untyped_object.region.end

class KernelObjectAllocator:
    """Allocator for kernel objects.

    This tracks the space available in a set of untyped objects.
    On allocation an untyped with sufficient remaining space is
    returned (while updating the internal tracking).

    Within an untyped object this mimics the kernel's allocation
    policy (basically a bump allocator with alignment).

    The only 'choice' this allocator has is which untyped object
    to use. The current algorithm is simply first fit: the first
    untyped that has sufficient space. This is not optimal.

    Note: The allocator does not generate the Retype invocations;
    this must be done with more knowledge (specifically the destination
    cap) which is distinct.

    It is critical that invocations are generated in the same order
    as the allocations are made.

    """
    def __init__(self, kernel_boot_info: KernelBootInfo) -> None:
        self._allocation_idx = 0
        self._untyped = []
        for ut in kernel_boot_info.untyped_objects:
            if ut.is_device:
                # Kernel allocator can only allocate out of normal memory
                # device memory can't be used for kernel objects
                continue
            self._untyped.append(UntypedAllocator(ut, 0, []))

    def alloc(self, size: int, count: int = 1) -> KernelAllocation:
        assert is_power_of_two(size)
        for ut in self._untyped:
            # See if this fits
            start = round_up(ut.base + ut.allocation_point, size)
            if start + (count * size) <= ut.end:
                ut.allocation_point = (start - ut.base) + (count * size)
                self._allocation_idx += 1
                allocation = KernelAllocation(ut.untyped_object.cap, start, self._allocation_idx)
                ut.allocations.append(allocation)
                return allocation

        raise Exception("Can't alloc - nos pace")


def invocation_to_str(inv: Sel4Invocation, cap_lookup: Dict[int, str]) -> str:
    arg_strs = []
    for nm, val in inv._args:
        if nm in inv._extra_caps:
            val_str = f"0x{val:016x} ({cap_lookup.get(val)})"
            nm = f"{nm} (cap)"
        elif nm == "src_obj":
            # This is a special cap
            val_str = f"0x{val:016x} ({cap_lookup.get(val)})"
            nm = f"{nm} (cap)"
        elif nm == "vaddr":
            val_str = hex(val)
        elif nm == "size_bits":
            if val == 0:
                val_str = f"{val} (N/A)"
            else:
                val_str = f"{val} (0x{1 << val:x})"
        elif nm == "object_type":
            object_size = FIXED_OBJECT_SIZES.get(val)
            object_type_name = SEL4_OBJECT_TYPE_NAMES[val]
            if object_size is None:
                val_str = f"{val} ({object_type_name} - variable size)"
            else:
                val_str = f"{val} ({object_type_name} - 0x{object_size:x})"
        else:
            val_str = str(val)
        arg_strs.append(f"         {nm:20s} {val_str}")
    if hasattr(inv, "_repeat_count"):
        arg_strs.append(f"      REPEAT: count={inv._repeat_count} {inv._repeat_incr}")
    args = "\n".join(arg_strs)
    return f"{inv._object_type:20s} - {inv._method_name:17s} - 0x{inv._service:016x} ({cap_lookup.get(inv._service)})\n{args}"


def overlaps(range1: Tuple[int, int], range2: Tuple[int, int]) -> bool:
    """Return true if range1 overlaps range2"""
    base1, size1 = range1
    base2, size2 = range2
    if base1 >= base2 + size2:
        # range1 is completely above range2
        return False
    if  base1 + size1 <= base2:
        # range1 is completely below range2
        return False
    # otherwise there is some overlap
    return True



def phys_mem_regions_from_elf(elf: ElfFile, alignment: int) -> List[MemoryRegion]:
    """Determine the physical memory regions for an ELF file with a given
    alignment.

    The returned region shall be extended (if necessary) so that the start
    and end are congruent with the specified alignment (usually a page size).
    """
    assert alignment > 0
    return [
        MemoryRegion(
            round_down(segment.phys_addr, alignment),
            round_up(segment.phys_addr + len(segment.data), alignment)
        )
        for segment in elf.segments
    ]


def phys_mem_region_from_elf(elf: ElfFile, alignment: int) -> MemoryRegion:
    """Determine a single physical memory region for an ELF.

    Works as per phys_mem_regions_from_elf, but checks the ELF has a single
    segment, and returns the region covering the first segment.
    """
    assert alignment > 0
    assert len(elf.segments) == 1
    return phys_mem_regions_from_elf(elf, alignment)[0]


def virt_mem_regions_from_elf(elf: ElfFile, alignment: int) -> List[MemoryRegion]:
    """Determine the virtual memory regions for an ELF file with a given
    alignment.

    The returned region shall be extended (if necessary) so that the start
    and end are congruent with the specified alignment (usually a page size).
    """
    assert alignment > 0
    return [
        MemoryRegion(
            round_down(segment.virt_addr, alignment),
            round_up(segment.virt_addr + len(segment.data), alignment)
        )
        for segment in elf.segments
    ]


def virt_mem_region_from_elf(elf: ElfFile, alignment: int) -> MemoryRegion:
    """Determine a single virtual memory region for an ELF.

    Works as per virt_mem_regions_from_elf, but checks the ELF has a single
    segment, and returns the region covering the first segment.
    """
    assert alignment > 0
    assert len(elf.segments) == 1
    return virt_mem_regions_from_elf(elf, alignment)[0]


class PageOverlap(Exception):
    pass


class FixedUntypedAlloc:
    def __init__(self, ut: UntypedObject) -> None:
        self._ut = ut
        self.watermark = self._ut.base

    def __lt__(self, other: "FixedUntypedAlloc") -> bool:
        return self._ut.region.base < other._ut.region.base

    def __str__(self) -> str:
        return f"FixedUntypedAlloc(self._ut={self._ut}"

    def __repr__(self) -> str:
        return str(self)

    def __contains__(self, address: int) -> bool:
        return self._ut.region.base <= address < self._ut.region.end

@dataclass(frozen=True, eq=True)
class KernelObject:
    """Represents an allocated kernel object.

    object_type is the type of kernel object.
    phys_address is the physical memory address of the kernel object.

    Kernel objects can have multiple caps (and caps can have multiple addresses).
    The cap referred to here is the original cap that is allocated when the
    kernel object is first allocate.
    The cap_slot refers to the specific slot in which this cap resides.
    The cap_address refers to a cap address that addresses this cap.
    The cap_address is is intended to be valid within the context of the
    initial task.
    """
    object_type: int
    cap_slot: int
    cap_addr: int
    phys_addr: int
    name: str


def assert_objects_adjacent(lst: List[KernelObject]) -> None:
    """check that all objects in the list are adjacent"""
    prev_cap_addr = lst[0].cap_addr
    for o in lst[1:]:
        assert o.cap_addr == prev_cap_addr + 1
        prev_cap_addr = o.cap_addr


def human_size_strict(size: int) -> str:
    """Product a 'human readable' string for the size.

    'strict' means that it must be simply represented.
    Specifically, it must be a multiple of standard power-of-two.
    (e.g. KiB, MiB, GiB, TiB, PiB, EiB)
    """
    if size > (1 << 70):
        raise ValueError("size is too large for human representation")
    for bits, label in (
        (60, "EiB"),
        (50, "PiB"),
        (40, "TiB"),
        (30, "GiB"),
        (20, "MiB"),
        (10, "KiB"),
        (0, "bytes"),
    ):
        base = 1 << bits
        if size > base:
            if base > 0:
                count, extra = divmod(size, base)
                if extra != 0:
                    raise ValueError(f"size 0x{size:x} is not a multiple of standard power-of-two")
            else:
                count = size
            return f"{count:,d} {label}"
    raise Exception("should never reach here")


class InitSystem:
    def __init__(
            self,
            kernel_config: KernelConfig,
            cnode_cap: int,
            cnode_mask: int,
            first_available_cap_slot: int,
            kernel_object_allocator: KernelObjectAllocator,
            kernel_boot_info: KernelBootInfo,
            invocations: List[Sel4Invocation],
            cap_address_names: Dict[int, str],
        ):
        self._cnode_cap = cnode_cap
        self._cnode_mask = cnode_mask
        self._kernel_config = kernel_config
        self._kao = kernel_object_allocator
        self._invocations = invocations
        self._cap_slot = first_available_cap_slot
        self._last_fixed_address = 0
        self._device_untyped = sorted([FixedUntypedAlloc(ut) for ut in kernel_boot_info.untyped_objects if ut.is_device])
        self._cap_address_names = cap_address_names
        self._objects: List[KernelObject] = []

    def reserve(self, allocations: List[Tuple[UntypedObject, int]]) -> None:
        for alloc_ut, alloc_phys_addr in allocations:
            for ut in self._device_untyped:
                if alloc_ut == ut._ut:
                    break
            else:
                raise Exception(f"Allocation {alloc_ut} ({alloc_phys_addr:x}) not in any device untyped")

            if not (ut._ut.region.base <= alloc_phys_addr <= ut._ut.region.end):
                raise Exception(f"Allocation {alloc_ut} ({alloc_phys_addr:x}) not in untyped region {ut._ut.region}")

            ut.watermark = alloc_phys_addr


    def allocate_fixed_objects(self, phys_address: int, object_type: int, count: int, names: List[str]) -> List[KernelObject]:
        """

        Note: Fixed objects must be allocated in order!
        """
        assert phys_address >= self._last_fixed_address
        assert object_type in FIXED_OBJECT_SIZES
        assert count == len(names)
        alloc_size = FIXED_OBJECT_SIZES[object_type]

        for ut in self._device_untyped:
            if phys_address in ut:
                break
        else:
            for ut in self._device_untyped:
                print(ut)
            raise Exception(f"{phys_address=:x} not in any device untyped")

        if phys_address < ut.watermark:
            raise Exception(f"{phys_address=:x} is below watermark")

        if ut.watermark != phys_address:
            # If the watermark isn't at the right spot, then we need to
            # create padding objects until it is.
            padding_required = phys_address - ut.watermark
            # We are restricted in how much we can pad:
            # 1: Untyped objects must be power-of-two sized.
            # 2: Untyped objects must be aligned to their size.
            padding_sizes = []
            # We have two potential approaches for how we pad.
            # 1: Use largest objects possible respecting alignment
            # and size restrictions.
            # 2: Use a fixed size object multiple times. This will
            # create more objects, but as same sized objects can be
            # create in a batch, required fewer invocations.
            # For now we choose #1
            wm = ut.watermark
            while padding_required > 0:
                wm_lsb = lsb(wm)
                sz_msb = msb(padding_required)
                pad_object_size = 1 << min(wm_lsb, sz_msb)
                padding_sizes.append(pad_object_size)
                wm += pad_object_size
                padding_required -= pad_object_size

            for sz in padding_sizes:
                self._invocations.append(Sel4UntypedRetype(
                        ut._ut.cap,
                        SEL4_UNTYPED_OBJECT,
                        int(log2(sz)),
                        self._cnode_cap,
                        1,
                        1,
                        self._cap_slot,
                        1
                ))
                self._cap_slot += 1

        object_cap = self._cap_slot
        self._cap_slot += 1
        self._invocations.append(Sel4UntypedRetype(
                ut._ut.cap,
                object_type,
                0,
                self._cnode_cap,
                1,
                1,
                object_cap,
                1
        ))

        ut.watermark = phys_address + alloc_size
        self._last_fixed_address = phys_address + alloc_size
        cap_address = self._cnode_mask | object_cap
        self._cap_address_names[cap_address] = names[0]
        kernel_objects = [KernelObject(object_type, object_cap, cap_address, phys_address, names[0])]
        self._objects += kernel_objects
        return kernel_objects

    def allocate_objects(self, object_type: int, names: List[str], size: Optional[int] = None) -> List[KernelObject]:
        count = len(names)
        if object_type in FIXED_OBJECT_SIZES:
            assert size is None
            alloc_size = FIXED_OBJECT_SIZES[object_type]
            api_size = 0
        elif object_type in (SEL4_CNODE_OBJECT, SEL4_SCHEDCONTEXT_OBJECT):
            assert size is not None
            assert is_power_of_two(size)
            api_size = int(log2(size))
            alloc_size = size * SLOT_SIZE
        else:
            raise Exception(f"Invalid object type: {object_type}")
        allocation = self._kao.alloc(alloc_size, count)
        base_cap_slot = self._cap_slot
        self._cap_slot += count
        to_alloc = count
        alloc_cap_slot = base_cap_slot
        while to_alloc:
            call_count = min(to_alloc, self._kernel_config.fan_out_limit)
            self._invocations.append(Sel4UntypedRetype(
                    allocation.untyped_cap_address,
                    object_type,
                    api_size,
                    self._cnode_cap,
                    1,
                    1,
                    alloc_cap_slot,
                    call_count
            ))
            to_alloc -= call_count
            alloc_cap_slot += call_count
        kernel_objects = []
        phys_addr = allocation.phys_addr
        for idx in range(count):
            cap_slot = base_cap_slot + idx
            cap_address = self._cnode_mask | cap_slot
            name = names[idx]
            self._cap_address_names[cap_address] = name
            kernel_objects.append(KernelObject(object_type, cap_slot, cap_address, phys_addr, name))
            phys_addr += alloc_size

        self._objects += kernel_objects
        return kernel_objects


@dataclass(frozen=True)
class Region:
    name: str
    addr: int
    data: bytearray

    def __repr__(self) -> str:
        return f"<Region name={self.name} addr=0x{self.addr:x} size={len(self.data)}>"


@dataclass
class BuiltSystem:
    number_of_system_caps: int
    invocation_data_size: int
    bootstrap_invocations: List[Sel4Invocation]
    system_invocations: List[Sel4Invocation]
    kernel_boot_info: KernelBootInfo
    reserved_region: MemoryRegion
    fault_ep_cap_address: int
    reply_cap_address: int
    cap_lookup: Dict[int, str]
    tcb_caps: List[int]
    regions: List[Region]
    kernel_objects: List[KernelObject]
    initial_task_virt_region: MemoryRegion
    initial_task_phys_region: MemoryRegion


def _get_full_path(filename: Path, search_paths: List[Path]) -> Path:
    for search_path in search_paths:
        full_path = search_path / filename
        if full_path.exists():
            return full_path
    else:
        raise UserError(f"Error: unable to find program image: '{filename}'")


def build_system(
        kernel_config: KernelConfig,
        kernel_elf: ElfFile,
        monitor_elf: ElfFile,
        system: SystemDescription,
        invocation_table_size: int,
        system_cnode_size: int,
        search_paths: List[Path],
    ) -> BuiltSystem:
    """Build system as description by the inputs, with a 'BuiltSystem' object as the output."""
    assert is_power_of_two(system_cnode_size)
    assert invocation_table_size % kernel_config.minimum_page_size == 0
    assert invocation_table_size <= MAX_SYSTEM_INVOCATION_SIZE

    invocation: Sel4Invocation

    cap_address_names = {}
    cap_address_names[INIT_NULL_CAP_ADDRESS] = "null"
    cap_address_names[INIT_TCB_CAP_ADDRESS] = "TCB: init"
    cap_address_names[INIT_CNODE_CAP_ADDRESS] = "CNode: init"
    cap_address_names[INIT_VSPACE_CAP_ADDRESS] = "VSpace: init"
    cap_address_names[INIT_ASID_POOL_CAP_ADDRESS] = "ASID Pool: init"
    cap_address_names[IRQ_CONTROL_CAP_ADDRESS] = "IRQ Control"

    system_cnode_bits = int(log2(system_cnode_size))

    # Emulate kernel boot

    virtual_machines = [pd.virtual_machine for pd in system.protection_domains if pd.virtual_machine is not None]

    ## Determine physical memory region used by the monitor
    initial_task_size = phys_mem_region_from_elf(monitor_elf, kernel_config.minimum_page_size).size

    ## Get the elf files for each pd:
    pd_elf_files = {
        pd: ElfFile.from_path(_get_full_path(pd.program_image, search_paths))
        for pd in system.protection_domains
    }
    vm_images = {
        vm: _get_full_path(vm.program_image, search_paths)
        for vm in virtual_machines
    }
    vm_device_trees = {
        vm: _get_full_path(vm.device_tree, search_paths)
        for vm in virtual_machines if vm.device_tree is not None
    }
    ### Here we should validate that ELF files @ivanv: this comment is weird ?

    ## Determine physical memory region for 'reserved' memory.
    #
    # The 'reserved' memory region will not be touched by seL4 during boot
    # and allows the monitor (initial task) to create memory regions
    # from this area, which can then be made available to the appropriate
    # protection domains
    pd_elf_size = sum([
        sum([r.size for r in phys_mem_regions_from_elf(elf, kernel_config.minimum_page_size)])
        for elf in pd_elf_files.values()
    ])
    vm_image_size = sum([
        round_up(os.path.getsize(image), kernel_config.minimum_page_size)
        for image in vm_images.values()
    ])
    vm_image_size += sum([
        round_up(os.path.getsize(device_tree), kernel_config.minimum_page_size)
        for device_tree in vm_device_trees.values() if device_tree is not None
    ])
    reserved_size = invocation_table_size + pd_elf_size + vm_image_size
    print(f"Reserved size: {reserved_size}")

    # Now that the size is determine, find a free region in the physical memory
    # space.
    available_memory = emulate_kernel_boot_partial(
        kernel_config,
        kernel_elf,
    )

    reserved_base = available_memory.allocate(reserved_size)
    initial_task_phys_base = available_memory.allocate(initial_task_size)
    # The kernel relies on this ordering. The previous allocation functions do *NOT* enforce
    # this though, should fix that.
    assert reserved_base < initial_task_phys_base

    initial_task_phys_region = MemoryRegion(initial_task_phys_base, initial_task_phys_base + initial_task_size)
    initial_task_virt_region = virt_mem_region_from_elf(monitor_elf, kernel_config.minimum_page_size)

    reserved_region = MemoryRegion(reserved_base, reserved_base + reserved_size)

    # Now that the reserved region has been allocated we can determine the specific
    # region of physical memory required for the inovcation table itself, and
    # all the ELF segments
    invocation_table_region = MemoryRegion(reserved_base, reserved_base + invocation_table_size)

    phys_addr_next = invocation_table_region.end
    # Now we create additional MRs (and mappings) for the ELF files.
    pd_elf_regions = {}
    for pd in system.protection_domains:
        elf_regions: List[Tuple[int, bytearray, str]] = []
        seg_idx = 0
        for segment in pd_elf_files[pd].segments:
            if not segment.loadable:
                continue

            perms = ""
            if segment.is_readable:
                perms += "r"
            if segment.is_writable:
                perms += "w"
            if segment.is_executable:
                perms += "x"

            elf_regions.append((phys_addr_next, segment.data, perms))
            phys_addr_next = round_up(phys_addr_next + len(segment.data), kernel_config.minimum_page_size)

            # base_vaddr = round_down(segment.virt_addr, kernel_config.minimum_page_size)
            # end_vaddr = round_up(segment.virt_addr + segment.mem_size, kernel_config.minimum_page_size)
            # aligned_size = end_vaddr - base_vaddr
            # name = f"ELF:{pd.name}-{seg_idx}"
            # mr = SysMemoryRegion(name, "small", aligned_size // kernel_config.minimum_page_size, phys_addr_next)
            # seg_idx += 1
            # phys_addr_next += aligned_size
            # system.mr_by_name[mr.name] = mr
            # system.memory_regions.append(mr)

            # mp = SysMap(mr.name, base_vaddr, perms=perms, cached=True)
            # pd.maps.append(mp)
        pd_elf_regions[pd] = tuple(elf_regions)

    for vm in virtual_machines:
        phys_addr_next += round_up(os.path.getsize(vm.program_image), kernel_config.minimum_page_size)
        if vm.device_tree is not None:
            phys_addr_next += round_up(os.path.getsize(vm.device_tree), kernel_config.minimum_page_size)


    # 1.3 With both the initial task region and reserved region determined the kernel
    # boot can be emulated. This provides the boot info information which is needed
    # for the next steps
    kernel_boot_info = emulate_kernel_boot(
        kernel_config,
        kernel_elf,
        initial_task_phys_region,
        initial_task_virt_region,
        reserved_region
    )

    for ut in kernel_boot_info.untyped_objects:
        dev_str = " (device)" if ut.is_device else ""
        cap_address_names[ut.cap] = f"Untyped @ 0x{ut.region.base:x}:0x{ut.region.size:x}{dev_str}"

    # X. The kernel boot info allows us to create an allocator for kernel objects
    kao = KernelObjectAllocator(kernel_boot_info)

    # 2. Now that the available resources are known it is possible to proceed with the
    # monitor task boot strap.
    #
    # The boot strap of the monitor works in two phases:
    #
    #   1. Setting up the monitor's CSpace
    #   2. Making the system invocation table available in the monitor's address
    #   space.

    # 2.1 The monitor's CSpace consists of two CNodes: a/ the initial task CNode
    # which consists of all the fixed initial caps along with caps for the
    # object create during kernel bootstrap, and b/ the system CNode, which
    # contains caps to all objects that will be created in this process.
    # The system CNode is of `system_cnode_size`. (Note: see also description
    # on how `system_cnode_size` is iteratively determined).
    #
    # The system CNode is not available at startup and must be created (by retyping
    # memory from an untyped object). Once created the two CNodes must be aranged
    # as a tree such that the slots in both CNodes are addressable.
    #
    # The system CNode shall become the root of the CSpace. The initial CNode shall
    # be copied to slot zero of the system CNode. In this manner all caps in the initial
    # CNode will keep their original cap addresses. This isn't required but it makes
    # allocation, debugging and reasoning about the system more straight forward.
    #
    # The guard shall be selected so the least significant bits are used. The guard
    # for the root shall be:
    #
    #   64 - system cnode bits - initial cnode bits
    #
    # The guard for the initial CNode will be zero.
    #
    # 2.1.1: Allocate the *root* CNode. It is two entries:
    #  slot 0: the existing init cnode
    #  slot 1: our main system cnode
    root_cnode_bits = 1
    root_cnode_allocation = kao.alloc((1 << root_cnode_bits) * (1 << SLOT_BITS))
    root_cnode_cap =  kernel_boot_info.first_available_cap
    cap_address_names[root_cnode_cap] = "CNode: root"

    # 2.1.2: Allocate the *system* CNode. It is the cnodes that
    # will have enough slots for all required caps.
    system_cnode_allocation = kao.alloc(system_cnode_size * (1 << SLOT_BITS))
    system_cnode_cap = kernel_boot_info.first_available_cap + 1
    cap_address_names[system_cnode_cap] = "CNode: system"

    # 2.1.3: Now that we've allocated the space for these we generate
    # the actual systems calls.
    #
    # First up create the root cnode
    bootstrap_invocations: List[Sel4Invocation] = []

    bootstrap_invocations.append(Sel4UntypedRetype(
            root_cnode_allocation.untyped_cap_address,
            SEL4_CNODE_OBJECT,
            root_cnode_bits,
            INIT_CNODE_CAP_ADDRESS,
            0,
            0,
            root_cnode_cap,
            1
    ))

    # 2.1.4: Now insert a cap to the initial Cnode into slot zero of the newly
    # allocated root Cnode. It uses sufficient guard bits to ensure it is
    # completed padded to word size
    #
    # guard size is the lower bit of the guard, upper bits are the guard itself
    # which for out purposes is always zero.
    guard = kernel_config.cap_address_bits - root_cnode_bits - kernel_config.root_cnode_bits
    bootstrap_invocations.append(Sel4CnodeMint(
        root_cnode_cap,
        0,
        root_cnode_bits,
        INIT_CNODE_CAP_ADDRESS,
        INIT_CNODE_CAP_ADDRESS,
        kernel_config.cap_address_bits,
        SEL4_RIGHTS_ALL,
        guard
    ))

    # 2.1.5: Now it is possible to switch our root Cnode to the newly create
    # root cnode. We have a zero sized guard. This Cnode represents the top
    # bit of any cap addresses.
    #
    root_guard = 0
    bootstrap_invocations.append(Sel4TcbSetSpace(
        INIT_TCB_CAP_ADDRESS,
        INIT_NULL_CAP_ADDRESS,
        root_cnode_cap,
        root_guard,
        INIT_VSPACE_CAP_ADDRESS,
        0
    ))

    # 2.1.6: Now we can create our new system Cnode. We will place it into
    # a temporary cap slot in the initial CNode to start with.
    bootstrap_invocations.append(Sel4UntypedRetype(
        system_cnode_allocation.untyped_cap_address,
        SEL4_CNODE_OBJECT,
        system_cnode_bits,
        INIT_CNODE_CAP_ADDRESS,
        0,
        0,
        system_cnode_cap,
        1
    ))

    # 2.1.7: Now that the we have create the object, we can 'mutate' it
    # to the correct place:
    # Slot #1 of the new root cnode
    guard = kernel_config.cap_address_bits - root_cnode_bits - system_cnode_bits
    system_cap_address_mask = 1 << (kernel_config.cap_address_bits - 1)
    bootstrap_invocations.append(Sel4CnodeMint(
        root_cnode_cap,
        1,
        root_cnode_bits,
        INIT_CNODE_CAP_ADDRESS,
        system_cnode_cap,
        kernel_config.cap_address_bits,
        SEL4_RIGHTS_ALL,
        guard
    ))

    # 2.2 At this point it is necessary to get the frames containing the
    # main system invocations into the virtual address space. (Remember the
    # invocations we are writing out here actually _execute_ at run time!
    # It is a bit weird that we talk about mapping in the invocation data
    # before we have even generated the invocation data!).
    #
    # This needs a few steps:
    #
    # 1. Turn untyped into page objects
    # 2. Map the page objects into the address space
    #

    # 2.2.1: The memory for the system invocation data resides at the start
    # of the reserved region. We can retype multiple frames as a time (
    # which reduces the number of invocations we need). However, it is possible
    # that the region spans multiple untyped objects.
    # At this point in time we assume we will map the area using the minimum
    # page size. It would be good in the future to use super pages (when
    # it makes sense to - this would reduce memory usage, and the number of
    # invocations required to set up the address space
    pages_required= invocation_table_size // kernel_config.minimum_page_size
    remaining_pages = pages_required
    invocation_table_allocations = []
    phys_addr = invocation_table_region.base
    base_page_cap = 0
    for pta in range(base_page_cap, base_page_cap + pages_required):
        cap_address_names[system_cap_address_mask | pta] = "SmallPage: monitor invocation table"

    cap_slot = base_page_cap
    for ut in (ut for ut in kernel_boot_info.untyped_objects if ut.is_device):
        ut_pages = ut.region.size // kernel_config.minimum_page_size
        retype_page_count = min(ut_pages, remaining_pages)
        if retype_page_count > kernel_config.fan_out_limit:
            print(f"retype_page_count: {retype_page_count}, fan_out_limit: {kernel_config.fan_out_limit}")
        assert retype_page_count <= kernel_config.fan_out_limit
        bootstrap_invocations.append(Sel4UntypedRetype(
                ut.cap,
                SEL4_SMALL_PAGE_OBJECT,
                0,
                root_cnode_cap,
                1,
                1,
                cap_slot,
                retype_page_count
        ))

        remaining_pages -= retype_page_count
        cap_slot += retype_page_count
        phys_addr += retype_page_count * kernel_config.minimum_page_size
        invocation_table_allocations.append((ut, phys_addr))
        if remaining_pages == 0:
            break

    # 2.2.1: Now that physical pages have been allocated it is possible to setup
    # the virtual memory objects so that the pages can be mapped into virtual memory
    # At this point we map into the arbitrary address of 0x0.8000.0000 (i.e.: 2GiB)
    # We arbitrary limit the maximum size to be 128MiB. This allows for at least 1 million
    # invocations to occur at system startup. This should be enough for any reasonable
    # sized system.
    #
    # Before mapping it is necessary to install page tables that can cover the region.
    page_tables_required = round_up(invocation_table_size, SEL4_LARGE_PAGE_SIZE) // SEL4_LARGE_PAGE_SIZE
    page_table_allocation = kao.alloc(SEL4_PAGE_TABLE_SIZE, page_tables_required)
    base_page_table_cap = cap_slot

    for pta in range(base_page_table_cap, base_page_table_cap + page_tables_required):
        cap_address_names[system_cap_address_mask | pta] = "PageTable: monitor"

    assert page_tables_required <= kernel_config.fan_out_limit
    bootstrap_invocations.append(Sel4UntypedRetype(
            page_table_allocation.untyped_cap_address,
            SEL4_PAGE_TABLE_OBJECT,
            0,
            root_cnode_cap,
            1,
            1,
            cap_slot,
            page_tables_required
    ))
    cap_slot += page_tables_required

    # Now that the page tables are allocated they can be mapped into vspace
    vaddr = 0x8000_0000
    invocation = Sel4PageTableMap(system_cap_address_mask | base_page_table_cap,
                                  INIT_VSPACE_CAP_ADDRESS,
                                  vaddr,
                                  SEL4_ARM_DEFAULT_VMATTRIBUTES)
    invocation.repeat(page_tables_required, page_table=1, vaddr=SEL4_LARGE_PAGE_SIZE)
    bootstrap_invocations.append(invocation)

    # Finally, once the page tables are allocated the pages can be mapped
    vaddr = 0x8000_0000
    invocation = Sel4PageMap(system_cap_address_mask | base_page_cap,
                             INIT_VSPACE_CAP_ADDRESS,
                             vaddr,
                             SEL4_RIGHTS_READ,
                             SEL4_ARM_DEFAULT_VMATTRIBUTES | SEL4_ARM_EXECUTE_NEVER)
    invocation.repeat(pages_required, page=1, vaddr=kernel_config.minimum_page_size)
    bootstrap_invocations.append(invocation)


    # 3. Now we can start setting up the system based on the information
    # the user provided in the system xml.
    #
    # Create all the objects:
    #
    #  TCBs: one per PD
    #  Endpoints: one per PD with a PP + one for the monitor
    #  Notification: one per PD
    #  VSpaces: one per PD
    #  CNodes: one per PD
    #  Small Pages:
    #     one per pd for IPC buffer
    #     as needed by MRs
    #  Large Pages:
    #     as needed by MRs
    #  Page table structs:
    #     as needed by protection domains based on mappings required


    phys_addr_next = reserved_base + invocation_table_size
    # Now we create additional MRs (and mappings) for the ELF files.
    regions: List[Region] = []
    extra_mrs = []
    pd_extra_maps: Dict[ProtectionDomain, Tuple[SysMap, ...]] = {pd: tuple() for pd in list(system.protection_domains) + virtual_machines}
    for pd in list(system.protection_domains):
        seg_idx = 0
        for segment in pd_elf_files[pd].segments:
            if not segment.loadable:
                continue

            regions.append(Region(f"PD-ELF {pd.name}-{seg_idx}", phys_addr_next, segment.data))

            perms = ""
            if segment.is_readable:
                perms += "r"
            if segment.is_writable:
                perms += "w"
            if segment.is_executable:
                perms += "x"

            base_vaddr = round_down(segment.virt_addr, kernel_config.minimum_page_size)
            end_vaddr = round_up(segment.virt_addr + segment.mem_size, kernel_config.minimum_page_size)
            aligned_size = end_vaddr - base_vaddr
            name = f"ELF:{pd.name}-{seg_idx}"
            mr = SysMemoryRegion(name, aligned_size, 0x1000, aligned_size // 0x1000, phys_addr_next)
            seg_idx += 1
            phys_addr_next += aligned_size
            extra_mrs.append(mr)

            mp = SysMap(mr.name, base_vaddr, perms=perms, cached=True, element=None)
            pd_extra_maps[pd] += (mp, )

    for vm in virtual_machines:
        with open(vm.program_image, "rb") as f:
            data = f.read()

        regions.append(Region(f"VM-IMAGE {vm.name}", phys_addr_next, data))
        aligned_size = round_up(os.path.getsize(vm_images[vm]), kernel_config.minimum_page_size)
        mr = SysMemoryRegion(f"IMAGE:{vm.name}", aligned_size, 0x1000, aligned_size // 0x1000, phys_addr_next)
        phys_addr_next += aligned_size
        extra_mrs.append(mr)

        mp = SysMap(mr.name, 0x40080000, perms="rwx", cached=False, element=None) # @ivanv: fix
        pd_extra_maps[vm] += (mp, )

        if vm.device_tree:
            with open(vm.device_tree, "rb") as f:
                data = f.read()

            regions.append(Region(f"VM-DTB {vm.name}", phys_addr_next, data))
            aligned_size = round_up(os.path.getsize(vm_device_trees[vm]), kernel_config.minimum_page_size)
            mr = SysMemoryRegion(f"DTB:{vm.name}", aligned_size, 0x1000, aligned_size // 0x1000, phys_addr_next)
            phys_addr_next += aligned_size
            extra_mrs.append(mr)

            mp = SysMap(mr.name, 0x4f000000, perms="rwx", cached=False, element=None) # @ivanv: fix
            pd_extra_maps[vm] += (mp, )

    all_mrs = system.memory_regions + tuple(extra_mrs)
    all_mr_by_name = {mr.name: mr for mr in all_mrs}

    system_invocations: List[Sel4Invocation] = []
    init_system = InitSystem(kernel_config,
                             root_cnode_cap,
                             system_cap_address_mask,
                             cap_slot,
                             kao,
                             kernel_boot_info,
                             system_invocations,
                             cap_address_names)
    init_system.reserve(invocation_table_allocations)

    SUPPORTED_PAGE_SIZES = (0x1_000, 0x200_000)
    SUPPORTED_PAGE_OBJECTS = (SEL4_SMALL_PAGE_OBJECT, SEL4_LARGE_PAGE_OBJECT)
    PAGE_OBJECT_BY_SIZE = dict(zip(SUPPORTED_PAGE_SIZES, SUPPORTED_PAGE_OBJECTS))
    # 3.1 Work out how many regular (non-fixed) page objects are required
    page_names_by_size: Dict[int, List[str]] = {
        page_size: [] for page_size in SUPPORTED_PAGE_SIZES
    }
    page_names_by_size[0x1000] += [f"Page({human_size_strict(0x1000)}): IPC Buffer PD={pd.name}" for pd in system.protection_domains]
    for mr in all_mrs:
        if mr.phys_addr is not None:
            continue
        page_size_human = human_size_strict(mr.page_size)
        page_names_by_size[mr.page_size] +=  [f"Page({page_size_human}): MR={mr.name} #{idx}" for idx in range(mr.page_count)]

    page_objects: Dict[int, List[KernelObject]] = {}

    for page_size, page_object in reversed(list(zip(SUPPORTED_PAGE_SIZES, SUPPORTED_PAGE_OBJECTS))):
        page_objects[page_size] = init_system.allocate_objects(page_object, page_names_by_size[page_size])

    ipc_buffer_objects = page_objects[0x1000][:len(system.protection_domains)]

    # @ivanv: revisit this for VM
    pg_idx: Dict[int, int] = {sz: 0 for sz in SUPPORTED_PAGE_SIZES}
    pg_idx[0x1000] = len(system.protection_domains)
    mr_pages: Dict[SysMemoryRegion, List[KernelObject]] = {mr: [] for mr in all_mrs}
    for mr in all_mrs:
        if mr.phys_addr is not None:
            continue
        idx = pg_idx[mr.page_size]
        mr_pages[mr] = [page_objects[mr.page_size][i] for i in range(idx, idx + mr.page_count)]
        pg_idx[mr.page_size] += mr.page_count

    # 3.2 Now allocate all the fixed mRs

    # First we need to find all the requested pages and sorted them
    fixed_pages = []
    for mr in all_mrs: #system.memory_regions:
        if mr.phys_addr is None:
            continue
        phys_addr = mr.phys_addr
        for idx in range(mr.page_count):
            fixed_pages.append((phys_addr, mr))
            phys_addr += mr_page_bytes(mr)

    fixed_pages.sort()

    # FIXME: At this point we can recombine them into
    # groups to optimize allocation

    for phys_addr, mr in fixed_pages:
        if mr.page_size not in SUPPORTED_PAGE_SIZES:
            raise Exception(f"Invalid page_size: 0x{mr.page_size:x} for mr {mr}")
        obj_type = PAGE_OBJECT_BY_SIZE[mr.page_size]
        obj_type_name = f"Page({human_size_strict(mr.page_size)})"
        name = f"{obj_type_name}: MR={mr.name} @ {phys_addr:x}"
        page = init_system.allocate_fixed_objects(phys_addr, obj_type, 1, names=[name])[0]
        mr_pages[mr].append(page)

    # TCBs
    tcb_names = [f"TCB: PD={pd.name}" for pd in system.protection_domains]
    tcb_names += [f"TCB: VM={vm.name}" for vm in virtual_machines]
    tcb_objects = init_system.allocate_objects(SEL4_TCB_OBJECT, tcb_names)
    tcb_caps = [tcb_obj.cap_addr for tcb_obj in tcb_objects]
    # VCPUs
    vcpu_names = [f"VCPU: VM={vm.name}" for vm in virtual_machines]
    vcpu_objects = init_system.allocate_objects(SEL4_VCPU_OBJECT, vcpu_names)
    # SchedContexts
    schedcontext_names = [f"SchedContext: PD={pd.name}" for pd in system.protection_domains]
    schedcontext_names += [f"SchedContext: VM={vm.name}" for vm in virtual_machines]
    schedcontext_objects = init_system.allocate_objects(SEL4_SCHEDCONTEXT_OBJECT, schedcontext_names, size=PD_SCHEDCONTEXT_SIZE)
    # Endpoints
    pds_with_endpoints = [pd for pd in system.protection_domains if pd.needs_ep]
    endpoint_names = ["EP: Monitor Fault"] + [f"EP: PD={pd.name}" for pd in pds_with_endpoints]
    # Replies
    reply_names = ["Reply: Monitor"]+ [f"Reply: PD={pd.name}" for pd in system.protection_domains]
    reply_objects = init_system.allocate_objects(SEL4_REPLY_OBJECT, reply_names)
    reply_object = reply_objects[0]
    # FIXME: Probably only need reply objects for PPs
    pd_reply_objects = reply_objects[1:]
    endpoint_objects = init_system.allocate_objects(SEL4_ENDPOINT_OBJECT, endpoint_names)
    fault_ep_endpoint_object = endpoint_objects[0]
    pd_endpoint_objects = dict(zip(pds_with_endpoints, endpoint_objects[1:]))
    notification_names = [f"Notification: PD={pd.name}" for pd in system.protection_domains]
    notification_objects = init_system.allocate_objects(SEL4_NOTIFICATION_OBJECT, notification_names)
    notification_objects_by_pd = dict(zip(system.protection_domains, notification_objects))

    # Determine number of upper directory / directory / page table objects required
    #
    # Upper directory (level 3 table) is based on how many 512 GiB parts of the address
    # space is covered (normally just 1!).
    #
    # Page directory (level 2 table) is based on how many 1,024 MiB parts of
    # the address space is covered
    #
    # Page table (level 3 table) is based on how many 2 MiB parts of the
    # address space is covered (excluding any 2MiB regions covered by large
    # pages).

    uds = []
    ds = []
    pts = []
    for pd_idx, pd in enumerate(list(system.protection_domains) + virtual_machines):
        if pd_idx < len(system.protection_domains):
            ipc_buffer_vaddr, _ = pd_elf_files[pd].find_symbol("__sel4_ipc_buffer_obj")
        upper_directory_vaddrs = set()
        directory_vaddrs = set()
        page_table_vaddrs = set()

        # For each page, in each map determine we determine
        # which upper directory, directory and page table is resides
        # in, and then page sure this is set
        if pd_idx < len(system.protection_domains):
            vaddrs = [(ipc_buffer_vaddr, 0x1000)]
        else:
            vaddrs = []
        for map in (pd.maps + pd_extra_maps[pd]):
            mr = all_mr_by_name[map.mr]
            vaddr = map.vaddr
            for _ in range(mr.page_count):
                vaddrs.append((vaddr, mr.page_size))
                vaddr += mr_page_bytes(mr)

        for vaddr, page_size in vaddrs:
            upper_directory_vaddrs.add(mask_bits(vaddr, 12 + 9 + 9 + 9))
            directory_vaddrs.add(mask_bits(vaddr, 12 + 9 + 9))
            if page_size == 0x1_000:
                page_table_vaddrs.add(mask_bits(vaddr, 12 + 9))
        if not kernel_config.hyp_mode:
            uds += [(pd_idx, vaddr) for vaddr in sorted(upper_directory_vaddrs)]
        ds += [(pd_idx, vaddr) for vaddr in sorted(directory_vaddrs)]
        pts += [(pd_idx, vaddr) for vaddr in sorted(page_table_vaddrs)]

    pd_names = [pd.name for pd in system.protection_domains]
    vm_names = [vm.name for vm in virtual_machines]
    vspace_names = [f"VSpace: PD={pd.name}" for pd in system.protection_domains]
    vspace_names += [f"VSpace: VM={vm.name}" for vm in virtual_machines]
    vspace_objects = init_system.allocate_objects(SEL4_VSPACE_OBJECT, vspace_names)

    if not kernel_config.hyp_mode:
        ud_names = [f"PageUpperDirectory: PD={pd_names[pd_idx]} VADDR=0x{vaddr:x}" for pd_idx, vaddr in uds]
        ud_objects = init_system.allocate_objects(SEL4_PAGE_UPPER_DIRECTORY_OBJECT, ud_names)

    pd_ds = ds[:len(pd_names)]
    vm_ds = ds[len(pd_names):]
    # print(f"pd_ds: {pd_ds}")
    # print(f"vm_ds: {vm_ds}")
    # print(f"pd_names: {pd_names}")
    # print(f"vm_names: {vm_names}")
    d_names = [f"PageDirectory: PD={pd_names[pd_idx]} VADDR=0x{vaddr:x}" for pd_idx, vaddr in pd_ds]
    d_names += [f"PageDirectory: VM={vm_names[vm_idx - len(pd_ds)]} VADDR=0x{vaddr:x}" for vm_idx, vaddr in vm_ds]
    d_objects = init_system.allocate_objects(SEL4_PAGE_DIRECTORY_OBJECT, d_names)

    pd_pts = pts[:len(pd_names)]
    vm_pts = pts[len(pd_names):]
    pt_names = [f"PageTable: PD={pd_names[pd_idx]} VADDR=0x{vaddr:x}" for pd_idx, vaddr in pd_pts]
    pt_names += [f"PageTable: VM={vm_names[vm_idx - len(pd_ds)]} VADDR=0x{vaddr:x}" for vm_idx, vaddr in vm_pts]
    pt_objects = init_system.allocate_objects(SEL4_PAGE_TABLE_OBJECT, pt_names)

    # Create CNodes - all CNode objects are the same size: 128 slots.
    cnode_names = [f"CNode: PD={pd.name}" for pd in system.protection_domains]
    cnode_names += [f"CNode: VM={vm.name}" for vm in virtual_machines]
    cnode_objects = init_system.allocate_objects(SEL4_CNODE_OBJECT, cnode_names, size=PD_CAP_SIZE)
    # @ivanv: make a note why this is okay
    cnode_objects_by_pd = dict(zip(system.protection_domains, cnode_objects))

    cap_slot = init_system._cap_slot

    # Create all the necessary interrupt handler objects. These aren't
    # created through retype though!
    irq_cap_addresses: Dict[ProtectionDomain, List[int]] = {pd: [] for pd in system.protection_domains}
    for pd in system.protection_domains:
        for sysirq in pd.irqs:
            cap_address = system_cap_address_mask | cap_slot
            system_invocations.append(
                Sel4IrqControlGet(
                    IRQ_CONTROL_CAP_ADDRESS,
                    sysirq.irq,
                    root_cnode_cap,
                    cap_address,
                    kernel_config.cap_address_bits
                )
            )

            cap_slot += 1
            cap_address_names[cap_address] = f"IRQ Handler: irq={sysirq.irq:d}"
            irq_cap_addresses[pd].append(cap_address)

    # This has to be done prior to minting!
    # for vspace_obj in vspace_objects:
    #     system_invocations.append(Sel4AsidPoolAssign(INIT_ASID_POOL_CAP_ADDRESS, vspace_obj.cap_addr))
    invocation = Sel4AsidPoolAssign(INIT_ASID_POOL_CAP_ADDRESS, vspace_objects[0].cap_addr)
    invocation.repeat(len(system.protection_domains) + len(virtual_machines), vspace=1)
    system_invocations.append(invocation)

    # Create copies of all caps required via minting.

    # Mint copies of required pages, while also determing what's required
    # for later mapping
    page_descriptors = []
    for pd_idx, pd in enumerate(list(system.protection_domains) + virtual_machines):
        for mp in (pd.maps + pd_extra_maps[pd]):
            vaddr = mp.vaddr
            mr = all_mr_by_name[mp.mr] #system.mr_by_name[mp.mr]
            rights = 0
            attrs = SEL4_ARM_PARITY_ENABLED
            if "r" in mp.perms:
                rights |= SEL4_RIGHTS_READ
            if "w" in mp.perms:
                rights |= SEL4_RIGHTS_WRITE
            if "x" not in mp.perms:
                attrs |= SEL4_ARM_EXECUTE_NEVER
            if mp.cached:
                attrs |= SEL4_ARM_PAGE_CACHEABLE

            assert len(mr_pages[mr]) > 0
            assert_objects_adjacent(mr_pages[mr])

            invocation = Sel4CnodeMint(system_cnode_cap,
                                       cap_slot,
                                       system_cnode_bits,
                                       root_cnode_cap,
                                       mr_pages[mr][0].cap_addr,
                                       kernel_config.cap_address_bits,
                                       rights,
                                       0)
            invocation.repeat(len(mr_pages[mr]), dest_index=1, src_obj=1)
            system_invocations.append(invocation)

            page_descriptors.append((
                system_cap_address_mask | cap_slot,
                pd_idx,
                vaddr,
                rights,
                attrs,
                len(mr_pages[mr]),
                mr_page_bytes(mr)
            ))

            for idx in range(len(mr_pages[mr])):
                cap_address_names[system_cap_address_mask | (cap_slot + idx)] = cap_address_names[mr_pages[mr][0].cap_addr + idx] + " (derived)"

            cap_slot += len(mr_pages[mr])

    badged_irq_caps: Dict[ProtectionDomain, List[int]] = {pd: [] for pd in system.protection_domains}
    for notification_obj, pd in zip(notification_objects, system.protection_domains):
        for sysirq in pd.irqs:
            badge = 1 << sysirq.id_
            badged_cap_address = system_cap_address_mask | cap_slot
            system_invocations.append(
                Sel4CnodeMint(
                    system_cnode_cap,
                    cap_slot,
                    system_cnode_bits,
                    root_cnode_cap,
                    notification_obj.cap_addr,
                    kernel_config.cap_address_bits,
                    SEL4_RIGHTS_ALL,
                    badge)
            )
            cap_address_names[badged_cap_address] = cap_address_names[notification_obj.cap_addr] + f" (badge=0x{badge:x})"
            badged_irq_caps[pd].append(badged_cap_address)
            cap_slot += 1

    # Create a fault endpoint cap for each protection domain.
    # For root PDs this shall be the system fault_ep_endpoint_object.
    # For non-root PDs this shall be the parent endpoint.
    badged_fault_ep = system_cap_address_mask | cap_slot
    for idx, pd in enumerate(system.protection_domains, 1):
        is_root = pd.parent is None
        if is_root:
            fault_ep_cap = fault_ep_endpoint_object.cap_addr
            badge = idx
        else:
            assert pd.pd_id is not None
            assert pd.parent is not None
            fault_ep_cap = pd_endpoint_objects[pd.parent].cap_addr
            badge =  (1 << 62) | pd.pd_id

        invocation = Sel4CnodeMint(
            system_cnode_cap,
            cap_slot,
            system_cnode_bits,
            root_cnode_cap,
            fault_ep_cap,
            kernel_config.cap_address_bits,
            SEL4_RIGHTS_ALL,
            badge
        )
        system_invocations.append(invocation)
        cap_slot += 1

    # Create a fault endpoint cap for each virtual machine, this will
    # be the parent protection domain's endpoint.
    for idx, vm in enumerate(virtual_machines, 1):
        # @ivanv: this is inefficient, we should store the root PD
        # in the XML parsing instead
        # Find the PD that has the virtual machine
        for pd in system.protection_domains:
            if pd.virtual_machine == vm:
                parent_pd = pd
                break

        # print("===== parent_pd is:")
        # print(parent_pd)
        # print(pd_endpoint_objects)
        fault_ep_cap = pd_endpoint_objects[parent_pd].cap_addr
        # @ivanv: Right now there's nothing stopping the vm_id being
        # the same as a pd_id. We should change this.
        badge = (1 << 62) | vm.vm_id

        invocation = Sel4CnodeMint(
            system_cnode_cap,
            cap_slot,
            system_cnode_bits,
            root_cnode_cap,
            fault_ep_cap,
            kernel_config.cap_address_bits,
            SEL4_RIGHTS_ALL,
            badge
        )
        system_invocations.append(invocation)
        cap_slot += 1

    final_cap_slot = cap_slot

    ## Minting in the endpoint (or notification object if protected is not set)
    for pd, notification_obj, cnode_obj in zip(system.protection_domains, notification_objects, cnode_objects):
        obj = pd_endpoint_objects[pd] if pd.needs_ep else notification_obj
        assert INPUT_CAP_IDX < PD_CAP_SIZE
        system_invocations.append(
            Sel4CnodeMint(
                cnode_obj.cap_addr,
                INPUT_CAP_IDX,
                PD_CAP_BITS,
                root_cnode_cap,
                obj.cap_addr,
                kernel_config.cap_address_bits,
                SEL4_RIGHTS_ALL,
                0)
        )

    ## Mint access to reply cap
    assert REPLY_CAP_IDX < PD_CAP_SIZE
    invocation = Sel4CnodeMint(cnode_objects[0].cap_addr,
                               REPLY_CAP_IDX,
                               PD_CAP_BITS,
                               root_cnode_cap,
                               pd_reply_objects[0].cap_addr,
                               kernel_config.cap_address_bits,
                               SEL4_RIGHTS_ALL,
                               1)
    invocation.repeat(len(system.protection_domains), cnode=1, src_obj=1)
    system_invocations.append(invocation)

    ## Mint access to the vspace cap
    assert VSPACE_CAP_IDX < PD_CAP_SIZE
    invocation = Sel4CnodeMint(cnode_objects[0].cap_addr,
                               VSPACE_CAP_IDX,
                               PD_CAP_BITS,
                               root_cnode_cap,
                               vspace_objects[0].cap_addr,
                               kernel_config.cap_address_bits,
                               SEL4_RIGHTS_ALL,
                               0)
    invocation.repeat(len(system.protection_domains) + len(virtual_machines), cnode=1, src_obj=1)
    system_invocations.append(invocation)

    ## Mint access to interrupt handlers in the PD Cspace
    for cnode_obj, pd in zip(cnode_objects, system.protection_domains):
        for sysirq, irq_cap_address in zip(pd.irqs, irq_cap_addresses[pd]):
            cap_idx = BASE_IRQ_CAP + sysirq.id_
            assert cap_idx < PD_CAP_SIZE
            system_invocations.append(
                Sel4CnodeMint(
                    cnode_obj.cap_addr,
                    cap_idx,
                    PD_CAP_BITS,
                    root_cnode_cap,
                    irq_cap_address,
                    kernel_config.cap_address_bits,
                    SEL4_RIGHTS_ALL,
                    0)
            )

    ## Mint access to the child TCB in the PD Cspace
    for cnode_obj, pd in zip(cnode_objects, system.protection_domains):
        for maybe_child_tcb, maybe_child_pd in zip(tcb_objects, system.protection_domains):
            if maybe_child_pd.parent is pd:
                cap_idx = BASE_TCB_CAP + maybe_child_pd.pd_id
                system_invocations.append(
                    Sel4CnodeMint(
                        cnode_obj.cap_addr,
                        cap_idx,
                        PD_CAP_BITS,
                        root_cnode_cap,
                        maybe_child_tcb.cap_addr,
                        kernel_config.cap_address_bits,
                        SEL4_RIGHTS_ALL,
                        0)
                )

    ## Mint access to the VM's TCB in the PD Cspace
    for cnode_obj, pd in zip(cnode_objects, system.protection_domains):
        if pd.virtual_machine:
            for maybe_vm_tcb, maybe_vm in zip(tcb_objects[len(system.protection_domains):], virtual_machines):
                if pd.virtual_machine == maybe_vm:
                    cap_idx = BASE_TCB_CAP + maybe_vm.vm_id
                    print(f"Doing CNode mint for VM TCB, cap_idx: {cap_idx}")
                    system_invocations.append(
                        Sel4CnodeMint(
                            cnode_obj.cap_addr,
                            cap_idx,
                            PD_CAP_BITS,
                            root_cnode_cap,
                            maybe_vm_tcb.cap_addr,
                            kernel_config.cap_address_bits,
                            SEL4_RIGHTS_ALL,
                            0)
                    )

    for cc in system.channels:
        pd_a = system.pd_by_name[cc.pd_a]
        pd_b = system.pd_by_name[cc.pd_b]
        pd_a_cnode_obj = cnode_objects_by_pd[pd_a]
        pd_b_cnode_obj = cnode_objects_by_pd[pd_b]
        pd_a_notification_obj = notification_objects_by_pd[pd_a]
        pd_b_notification_obj = notification_objects_by_pd[pd_b]
        pd_a_endpoint_obj = pd_endpoint_objects.get(pd_a)
        pd_b_endpoint_obj = pd_endpoint_objects.get(pd_b)

        # Set up the notification baps
        pd_a_cap_idx = BASE_OUTPUT_NOTIFICATION_CAP + cc.id_a
        pd_a_badge = 1 << cc.id_b
        #pd_a.cnode.mint(pd_a_cap_idx, PD_CAPTABLE_BITS, sel4.init_cnode, pd_b.notification, 64, SEL4_RIGHTS_ALL, pd_a_badge)
        assert pd_a_cap_idx < PD_CAP_SIZE
        system_invocations.append(
            Sel4CnodeMint(
                pd_a_cnode_obj.cap_addr,
                pd_a_cap_idx,
                PD_CAP_BITS,
                root_cnode_cap,
                pd_b_notification_obj.cap_addr,
                kernel_config.cap_address_bits,
                SEL4_RIGHTS_ALL, # FIXME: Check rights
                pd_a_badge)
        )

        pd_b_cap_idx = BASE_OUTPUT_NOTIFICATION_CAP + cc.id_b
        pd_b_badge = 1 << cc.id_a
        #pd_b.cnode.mint(pd_b_cap_idx, PD_CAPTABLE_BITS, sel4.init_cnode, pd_a.notification, 64, SEL4_RIGHTS_ALL, pd_b_badge)
        assert pd_b_cap_idx < PD_CAP_SIZE
        system_invocations.append(
            Sel4CnodeMint(
                pd_b_cnode_obj.cap_addr,
                pd_b_cap_idx,
                PD_CAP_BITS,
                root_cnode_cap,
                pd_a_notification_obj.cap_addr,
                kernel_config.cap_address_bits,
                SEL4_RIGHTS_ALL, # FIXME: Check rights
                pd_b_badge)
        )

        # Set up the endpoint caps
        if pd_b.pp:
            pd_a_cap_idx = BASE_OUTPUT_ENDPOINT_CAP + cc.id_a
            pd_a_badge = (1 << 63) | cc.id_b
            # pd_a.cnode.mint(pd_a_cap_idx, PD_CAPTABLE_BITS, sel4.init_cnode, pd_b.endpoint, 64, SEL4_RIGHTS_ALL, pd_a_badge)
            assert pd_b_endpoint_obj is not None
            assert pd_a_cap_idx < PD_CAP_SIZE
            system_invocations.append(
                Sel4CnodeMint(
                    pd_a_cnode_obj.cap_addr,
                    pd_a_cap_idx,
                    PD_CAP_BITS,
                    root_cnode_cap,
                    pd_b_endpoint_obj.cap_addr,
                    kernel_config.cap_address_bits,
                    SEL4_RIGHTS_ALL, # FIXME: Check rights
                    pd_a_badge)
            )

        if pd_a.pp:
            pd_b_cap_idx = BASE_OUTPUT_ENDPOINT_CAP + cc.id_b
            pd_b_badge = (1 << 63) | cc.id_a
            #pd_b.cnode.mint(pd_b_cap_idx, PD_CAPTABLE_BITS, sel4.init_cnode, pd_a.endpoint, 64, SEL4_RIGHTS_ALL, pd_b_badge)
            assert pd_a_endpoint_obj is not None
            assert pd_b_cap_idx < PD_CAP_SIZE
            system_invocations.append(
                Sel4CnodeMint(
                    pd_b_cnode_obj.cap_addr,
                    pd_b_cap_idx,
                    PD_CAP_BITS,
                    root_cnode_cap,
                    pd_a_endpoint_obj.cap_addr,
                    kernel_config.cap_address_bits,
                    SEL4_RIGHTS_ALL, # FIXME: Check rights
                    pd_b_badge)
            )


    # All minting is complete at this point

    # Associate badges
    # FIXME: This could use repeat
    for notification_obj, pd in zip(notification_objects, system.protection_domains):
        for irq_cap_address, badged_notification_cap_address in zip(irq_cap_addresses[pd], badged_irq_caps[pd]):
            system_invocations.append(Sel4IrqHandlerSetNotification(irq_cap_address, badged_notification_cap_address))


    # Initialise the VSpaces -- assign them all the the initial asid pool.
    # print("pt_objects:")
    # print(pt_objects)
    for map_cls, descriptors, objects in [
        # (Sel4PageUpperDirectoryMap, uds, ud_objects), @ivanv change to get non-hypervisor mode working
        (Sel4PageDirectoryMap, ds, d_objects),
        (Sel4PageTableMap, pts, pt_objects),
    ]:
        for ((pd_idx, vaddr), obj) in zip(descriptors, objects):
            # print(f"Map invocation for vaddr: {hex(vaddr)}, pd_idx: {pd_idx}")
            vspace_obj = vspace_objects[pd_idx]
            system_invocations.append(
                map_cls(
                    obj.cap_addr,
                    vspace_obj.cap_addr,
                    vaddr,
                    SEL4_ARM_DEFAULT_VMATTRIBUTES
                )
            )

    # Now maps all the pages
    for page_cap_address, pd_idx, vaddr, rights, attrs, count, vaddr_incr in page_descriptors:
        vspace_obj = vspace_objects[pd_idx]
        invocation = Sel4PageMap(page_cap_address, vspace_obj.cap_addr, vaddr, rights, attrs)
        invocation.repeat(count, page=1, vaddr=vaddr_incr)
        system_invocations.append(invocation)

    # And, finally, map all the IPC buffers
    for vspace_obj, pd, ipc_buffer_obj in zip(vspace_objects, system.protection_domains, ipc_buffer_objects):
        vaddr, _ = pd_elf_files[pd].find_symbol("__sel4_ipc_buffer_obj")
        system_invocations.append(
            Sel4PageMap(
                ipc_buffer_obj.cap_addr,
                vspace_obj.cap_addr,
                vaddr,
                rights,
                attrs
            )
        )

    # Initialise the TCBs
    #
    # set scheduling parameters (SetSchedParams)
    for idx, (pd, schedcontext_obj) in enumerate(zip(list(system.protection_domains) + virtual_machines, schedcontext_objects)):
        # FIXME: We don't use repeat here because in the near future PDs will set the sched params
        system_invocations.append(
            Sel4SchedControlConfigureFlags(
                kernel_boot_info.schedcontrol_cap,
                schedcontext_obj.cap_addr,
                pd.budget,
                pd.period,
                0,
                0x100 + idx,
                0
            )
        )

    for tcb_obj, schedcontext_obj, pd in zip(tcb_objects, schedcontext_objects, list(system.protection_domains) + virtual_machines):
        system_invocations.append(Sel4TcbSetSchedParams(tcb_obj.cap_addr,
                                                        INIT_TCB_CAP_ADDRESS,
                                                        pd.priority,
                                                        pd.priority,
                                                        schedcontext_obj.cap_addr,
                                                        fault_ep_endpoint_object.cap_addr))

    # set vspace / cspace (SetSpace)
    invocation = Sel4TcbSetSpace(tcb_objects[0].cap_addr,
                                 badged_fault_ep,
                                 cnode_objects[0].cap_addr,
                                 kernel_config.cap_address_bits - PD_CAP_BITS,
                                 vspace_objects[0].cap_addr,
                                 0)
    invocation.repeat(len(system.protection_domains) + len(virtual_machines), tcb=1, fault_ep=1, cspace_root=1, vspace_root=1)
    system_invocations.append(invocation)

    # set IPC buffer
    for tcb_obj, pd, ipc_buffer_obj in zip(tcb_objects, system.protection_domains, ipc_buffer_objects):
        ipc_buffer_vaddr, _ = pd_elf_files[pd].find_symbol("__sel4_ipc_buffer_obj")
        system_invocations.append(Sel4TcbSetIpcBuffer(tcb_obj.cap_addr, ipc_buffer_vaddr, ipc_buffer_obj.cap_addr,))

    # print("PD ELF entries:")
    # for pd in list(system.protection_domains) + virtual_machines:
    #     print(pd)
    #     print(f"pd.entry: {hex(pd_elf_files[pd].entry)}")

    # set register (entry point)
    for tcb_obj, pd in zip(tcb_objects, system.protection_domains):
        system_invocations.append(
            Sel4TcbWriteRegisters(
                tcb_obj.cap_addr,
                False,
                0, # no flags on ARM
                Sel4Aarch64Regs(pc=pd_elf_files[pd].entry)
            )
        )
    # bind the notification object
    invocation = Sel4TcbBindNotification(tcb_objects[0].cap_addr, notification_objects[0].cap_addr)
    invocation.repeat(count=len(system.protection_domains), tcb=1, notification=1)
    system_invocations.append(invocation)

    # For all the virtual machines, we want to bind the TCB to the VCPU
    if len(virtual_machines) > 0:
        # print("TCB objects:")
        # print(tcb_objects)
        # print("TCB objects for VMs:")
        # print(tcb_objects[len(system.protection_domains)])
        invocation = Sel4ArmVcpuSetTcb(vcpu_objects[0].cap_addr, tcb_objects[len(system.protection_domains)].cap_addr)
        invocation.repeat(count=len(virtual_machines), vcpu=1, tcb=1)
        system_invocations.append(invocation)

    # Resume (start) all the threads that are not virtual machines
    invocation = Sel4TcbResume(tcb_objects[0].cap_addr)
    invocation.repeat(count=len(system.protection_domains), tcb=1)
    system_invocations.append(invocation)

    # All of the objects are created at this point; we don't need to both
    # the allocators from here.

    # And now we are done. We have all the invocations

    system_invocation_data = b''
    for system_invocation in system_invocations:
        system_invocation_data += system_invocation._get_raw_invocation(kernel_config)


    for pd in system.protection_domains:
        # Could use pd.elf_file.write_symbol here to update variables if required.
        pd_elf_files[pd].write_symbol("sel4cp_name", pack("<16s", pd.name.encode("utf8")))

    for pd in system.protection_domains:
        for setvar in pd.setvars:
            if setvar.region_paddr is not None:
                for mr in system.memory_regions:
                    if mr.name == setvar.region_paddr:
                        break
                else:
                    raise Exception(f"can't find region: {setvar.region_paddr}")
                value = mr_pages[mr][0].phys_addr
            elif setvar.vaddr is not None:
                value = setvar.vaddr
            try:
                pd_elf_files[pd].write_symbol(setvar.symbol, pack("<Q", value))
            except KeyError:
                raise Exception(f"Unable to patch variable '{setvar.symbol}' in protection domain: '{pd.name}': variable not found.")

    return BuiltSystem(
        number_of_system_caps = final_cap_slot, #init_system._cap_slot,
        invocation_data_size = len(system_invocation_data),
        bootstrap_invocations = bootstrap_invocations,
        system_invocations = system_invocations,
        kernel_boot_info = kernel_boot_info,
        reserved_region = reserved_region,
        fault_ep_cap_address = fault_ep_endpoint_object.cap_addr,
        reply_cap_address = reply_object.cap_addr,
        cap_lookup = cap_address_names,
        tcb_caps = tcb_caps,
        regions = regions,
        kernel_objects = init_system._objects,
        initial_task_phys_region = initial_task_phys_region,
        initial_task_virt_region = initial_task_virt_region,
    )


def main() -> int:
    if "SEL4CP_SDK" in environ:
        SDK_DIR = Path(environ["SEL4CP_SDK"])
    elif getattr(sys, 'oxidized', False):
        # If we a compiled binary we know where the root is
        SDK_DIR = Path(executable).parent.parent
    else:
        print("Error: SEL4CP_SDK must be set")
        return 1
    assert SDK_DIR is not None
    if not SDK_DIR.exists():
        print(f"Error: SDK directory '{SDK_DIR}' does not exist. Check SEL4CP_SDK environment variable is set correctly")
        return 1

    boards_path = SDK_DIR / "board"
    if not boards_path.exists():
        print(f"Error: SDK  directory '{SDK_DIR}' does not have a 'board' sub-directory. Check SEL4CP_SDK environment variable is set correctly")
        return 1

    available_boards = [p.name for p in boards_path.iterdir() if p.is_dir()]

    parser = ArgumentParser()
    parser.add_argument("system", type=Path)
    parser.add_argument("-o", "--output", type=Path, default=Path("loader.img"))
    parser.add_argument("-r", "--report", type=Path, default=Path("report.txt"))
    parser.add_argument("--board", required=True, choices=available_boards)
    parser.add_argument("--config", required=True)
    parser.add_argument("--search-path", nargs='*', type=Path)
    args = parser.parse_args()

    board_path = boards_path / args.board
    if not board_path.exists():
        print(f"Error: board path '{board_path}' doesn't exist.")
        return 1

    available_configs = [p.name for p in board_path.iterdir() if p.is_dir() and p.name != "example"]
    if args.config not in available_configs:
        parser.error(f"argument --config: invalid choice: '{args.config}' (choose from {available_configs})")

    gen_config_path = SDK_DIR / "board" / args.board / args.config / "config.yaml"
    elf_path = SDK_DIR / "board" / args.board / args.config / "elf"
    loader_elf_path = elf_path / "loader.elf"
    kernel_elf_path = elf_path / "sel4.elf"
    monitor_elf_path = elf_path / "monitor.elf"

    if not gen_config_path.exists():
        print(f"Error: auto-generated kernel config '{gen_config}' does not exist")
        return 1
    if not elf_path.exists():
        print(f"Error: board ELF directory '{elf_path}' does not exist")
        return 1
    if not loader_elf_path.exists():
        print(f"Error: loader ELF '{loader_elf_path}' does not exist")
        return 1
    if not kernel_elf_path.exists():
        print(f"Error: loader ELF '{kernel_elf_path}' does not exist")
        return 1
    if not monitor_elf_path.exists():
        print(f"Error: monitor ELF '{monitor_elf_path}' does not exist")
        return 1

    if not args.system.exists():
        print(f"Error: system description file '{args.system}' does not exist")
        return 1

    search_paths = [] if args.search_path is None else args.search_path
    search_paths.insert(0, Path.cwd())

    system_description = xml2system(args.system, default_platform_description)

    kernel_elf = ElfFile.from_path(kernel_elf_path)

    with open(gen_config_path, "r") as f:
        gen_config_yaml = yaml_load(f, Loader=YamlLoader)
    # What we really want is a dictionary that has every option as a key with it's correspoding value
    gen_config = {}
    for option_dict in gen_config_yaml:
        # @ivanv, so gross... we'll definitely have to find a better way
        option, value = list(option_dict.items())[0]
        gen_config[option] = value

    # Some of the options we need can be found in the auto-generated config YAML
    # file. Which we use here since they can differ between platforms.
    kernel_config = KernelConfig(
        word_size = gen_config["CONFIG_WORD_SIZE"],
        minimum_page_size = kb(4),
        paddr_user_device_top = gen_config["CONFIG_PADDR_USER_DEVICE_TOP"],
        kernel_frame_size = (1 << 12),
        root_cnode_bits = gen_config["CONFIG_ROOT_CNODE_SIZE_BITS"],
        cap_address_bits=64,
        fan_out_limit= gen_config["CONFIG_RETYPE_FAN_OUT_LIMIT"],
        hyp_mode = gen_config["CONFIG_ARM_HYPERVISOR_SUPPORT"],
    )

    monitor_elf = ElfFile.from_path(monitor_elf_path)
    if len(monitor_elf.segments) > 1:
        raise Exception("monitor ({monitor_elf_path}) has {len(monitor_elf.segments)} segments; must only have one")

    invocation_table_size = kernel_config.minimum_page_size
    system_cnode_size = 2

    while True:
        built_system = build_system(
            kernel_config,
            kernel_elf,
            monitor_elf,
            system_description,
            invocation_table_size,
            system_cnode_size,
            search_paths,
        )
        print(f"BUILT: {system_cnode_size=} {built_system.number_of_system_caps=} {invocation_table_size=} {built_system.invocation_data_size=}")
        if (built_system.number_of_system_caps <= system_cnode_size and
            built_system.invocation_data_size <= invocation_table_size):
            break

        # Recalculate the sizes for the next iteration
        new_invocation_table_size = round_up(built_system.invocation_data_size, kernel_config.minimum_page_size)
        new_system_cnode_size = 2 ** int(ceil(log2(built_system.number_of_system_caps)))

        invocation_table_size = max(invocation_table_size, new_invocation_table_size)
        system_cnode_size = max(system_cnode_size, new_system_cnode_size)

    # At this point we just need to patch the files (in memory) and write out the final image.

    # A: The monitor

    # A.1: As part of emulated boot we determined exactly how the kernel would
    # create untyped objects. Throught testing we know that this matches, but
    # we could have a bug, or the kernel could change. It that happens we are
    # in a bad spot! Things will break. So we write out this information so that
    # the monitor can double check this at run time.
    _, untyped_info_size = monitor_elf.find_symbol(MONITOR_CONFIG.untyped_info_symbol_name)
    max_untyped_objects = MONITOR_CONFIG.max_untyped_objects(untyped_info_size)
    if len(built_system.kernel_boot_info.untyped_objects) > max_untyped_objects:
        raise Exception(f"Too many untyped objects: monitor ({monitor_elf_path}) supports {max_untyped_objects:,d} regions."
                        f"System has {len(built_system.kernel_boot_info.untyped_objects):,d} objects.")
    untyped_info_header = MONITOR_CONFIG.untyped_info_header_struct.pack(
            built_system.kernel_boot_info.untyped_objects[0].cap,
            built_system.kernel_boot_info.untyped_objects[-1].cap + 1
        )
    untyped_info_object_data = []
    for idx, ut in enumerate(built_system.kernel_boot_info.untyped_objects):
        object_data = MONITOR_CONFIG.untyped_info_object_struct.pack(ut.base, ut.size_bits, ut.is_device)
        untyped_info_object_data.append(object_data)

    untyped_info_data = untyped_info_header + b''.join(untyped_info_object_data)
    monitor_elf.write_symbol(MONITOR_CONFIG.untyped_info_symbol_name, untyped_info_data)

    _, bootstrap_invocation_data_size = monitor_elf.find_symbol(MONITOR_CONFIG.bootstrap_invocation_data_symbol_name)

    bootstrap_invocation_data = b''
    for bootstrap_invocation in built_system.bootstrap_invocations:
        bootstrap_invocation_data += bootstrap_invocation._get_raw_invocation(kernel_config)

    if len(bootstrap_invocation_data) > bootstrap_invocation_data_size:
        print("INTERNAL ERROR: bootstrap invocations too large", file=stderr)
        print(f"bootstrap invocation array size   : {bootstrap_invocation_data_size:d}", file=stderr)
        print(f"bootstrap invocation required size: {len(bootstrap_invocation_data):d}", file=stderr)
        for bootstrap_invocation in built_system.bootstrap_invocations:
            print(invocation_to_str(bootstrap_invocation, built_system.cap_lookup), file=stderr)

        raise UserError("bootstrap invocations too large for monitor")

    monitor_elf.write_symbol(MONITOR_CONFIG.bootstrap_invocation_count_symbol_name, pack("<Q", len(built_system.bootstrap_invocations)))
    monitor_elf.write_symbol(MONITOR_CONFIG.system_invocation_count_symbol_name, pack("<Q", len(built_system.system_invocations)))
    monitor_elf.write_symbol(MONITOR_CONFIG.bootstrap_invocation_data_symbol_name, bootstrap_invocation_data)

    system_invocation_data = b''
    for system_invocation in built_system.system_invocations:
        system_invocation_data += system_invocation._get_raw_invocation(kernel_config)

    regions: List[Tuple[int, Union[bytes, bytearray]]] = [(built_system.reserved_region.base, system_invocation_data)]
    regions += [(r.addr, r.data) for r in built_system.regions]

    tcb_caps = built_system.tcb_caps
    monitor_elf.write_symbol("fault_ep", pack("<Q", built_system.fault_ep_cap_address))
    monitor_elf.write_symbol("reply", pack("<Q", built_system.reply_cap_address))
    monitor_elf.write_symbol("tcbs", pack("<Q" + "Q" * len(tcb_caps), 0, *tcb_caps))
    names_array = bytearray([0] * (64 * 16))
    for idx, pd in enumerate(system_description.protection_domains, 1):
        nm = pd.name.encode("utf8")[:15]
        names_array[idx * 16:idx * 16+len(nm)] = nm
    monitor_elf.write_symbol("pd_names", names_array)


    # B: The loader

    # B.1: The loader is primarily about loading 'regions' of memory.
    # so here we determine which regions it should be loading into
    # physical memory
    cap_lookup = built_system.cap_lookup

    # Reporting
    with args.report.open("w") as f:
        f.write("# Kernel Boot Info\n\n")
        f.write(f"    # of fixed caps     : {built_system.kernel_boot_info.fixed_cap_count:8,d}\n")
        f.write(f"    # of page table caps: {built_system.kernel_boot_info.paging_cap_count:8,d}\n")
        f.write(f"    # of page caps      : {built_system.kernel_boot_info.page_cap_count:8,d}\n")
        f.write(f"    # of untyped objects: {len(built_system.kernel_boot_info.untyped_objects):8,d}\n")
        f.write("\n")
        f.write("# Loader Regions\n\n")
        for region in built_system.regions:
            f.write(f"       {region}\n")
        f.write("\n")
        f.write("# Monitor (Initial Task) Info\n\n")
        f.write(f"     virtual memory : {built_system.initial_task_virt_region}\n")
        f.write(f"     physical memory: {built_system.initial_task_phys_region}\n")
        f.write("\n")
        f.write("# Allocated Kernel Objects Summary\n\n")
        f.write(f"     # of allocated objects: {len(built_system.kernel_objects):,d}\n")
        f.write("\n")
        f.write("# Bootstrap Kernel Invocations Summary\n\n")
        f.write(f"     # of invocations   : {len(built_system.bootstrap_invocations):10,d}\n")
        f.write(f"     size of invocations: {len(bootstrap_invocation_data):10,d}\n")
        f.write("\n")
        f.write("# System Kernel Invocations Summary\n\n")
        f.write(f"     # of invocations   : {len(built_system.system_invocations):10,d}\n")
        f.write(f"     size of invocations: {len(system_invocation_data):10,d}\n")
        f.write("\n")
        f.write("# Allocated Kernel Objects Detail\n\n")
        for ko in built_system.kernel_objects:
            f.write(f"    {ko.name:50s} {ko.object_type} cap_addr={ko.cap_addr:x} phys_addr={ko.phys_addr:x}\n")
        f.write("\n")
        f.write("# Bootstrap Kernel Invocations Detail\n\n")
        for idx, invocation in enumerate(built_system.bootstrap_invocations):
            f.write(f"    0x{idx:04x} {invocation_to_str(invocation, cap_lookup)}\n")
        f.write("\n")
        f.write("# System Kernel Invocations Detail\n\n")
        for idx, invocation in enumerate(built_system.system_invocations):
            f.write(f"    0x{idx:04x} {invocation_to_str(invocation, cap_lookup)}\n")

    # FIXME: Verify that the regions do not overlap!
    loader = Loader(
        loader_elf_path,
        kernel_elf,
        monitor_elf,
        built_system.initial_task_phys_region.base,
        built_system.reserved_region,
        regions,
        kernel_config
    )
    loader.write_image(args.output)

    return 0


if __name__ == "__main__":
    try:
        exit(main())
    except UserError as e:
        print(e)
        exit(1)
