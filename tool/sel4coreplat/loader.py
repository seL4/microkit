#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
from pathlib import Path
from struct import pack

from typing import Dict, List, Optional, Tuple, Union

from sel4coreplat.elf import ElfFile
from sel4coreplat.util import kb, mb, round_up, MemoryRegion
from sel4coreplat.sel4 import KernelConfig

AARCH64_1GB_BLOCK_BITS = 30
AARCH64_2MB_BLOCK_BITS = 21

AARCH64_LVL0_BITS = 9
AARCH64_LVL1_BITS = 9
AARCH64_LVL2_BITS = 9

PAGE_TABLE_SIZE = 4096

def mask(x: int) -> int:
    return ((1 << x) - 1)


def lvl0_index(addr: int) -> int:
    return (((addr) >> (AARCH64_2MB_BLOCK_BITS + AARCH64_LVL2_BITS + AARCH64_LVL1_BITS)) & mask(AARCH64_LVL0_BITS))


def lvl1_index(addr: int) -> int:
    return (((addr) >> (AARCH64_2MB_BLOCK_BITS + AARCH64_LVL2_BITS)) & mask(AARCH64_LVL1_BITS))


def lvl2_index(addr: int) -> int:
    return (((addr) >> (AARCH64_2MB_BLOCK_BITS)) & mask(AARCH64_LVL2_BITS))


def lvl0_addr(addr: int) -> int:
    bits = AARCH64_2MB_BLOCK_BITS + AARCH64_LVL2_BITS + AARCH64_LVL1_BITS
    return (addr >> bits) << bits


def lvl1_addr(addr: int) -> int:
    bits = AARCH64_2MB_BLOCK_BITS + AARCH64_LVL2_BITS
    return (addr >> bits) << bits


def lvl2_addr(addr: int) -> int:
    bits = AARCH64_2MB_BLOCK_BITS
    return (addr >> bits) << bits


def _check_non_overlapping(regions: List[Tuple[int, bytes]]) -> None:
    checked: List[Tuple[int, int]] = []
    for base, data in regions:
        end = base + len(data)
        # Check that this does not overlap any checked regions
        for b, e in checked:
            if not (end <= b or base >= e):
                raise Exception(f"Overlapping: {base:x}--{end:x} overlaps {b:x} -- {e:x}")

        checked.append((base, end))

class Loader:

    def __init__(self,
        loader_elf_path: Path,
        kernel_elf: ElfFile,
        initial_task_elf: ElfFile,
        initial_task_phys_base: Optional[int],
        reserved_region: MemoryRegion,
        regions: List[Tuple[int, bytes]],
        kernel_config: KernelConfig
    ) -> None:
        """

        Note: If initial_task_phys_base is not None, then it just this address
        as the base physical address of the initial task, rather than the address
        that comes from the initial_task_elf file.
        """
            # Setup the pagetable data structures (directly embedded in the loader)
        self._elf = ElfFile.from_path(loader_elf_path)
        sz = self._elf.word_size

        self._header_struct_fmt = "<IIIIIiIIII" if sz == 32 else "<QQQQQqQQQQ"
        self._region_struct_fmt = "<IIII" if sz == 32 else "<QQQQ"
        self._magic = 0x5e14dead if sz== 32 else 0x5e14dead14de5ead

        for loader_segment in self._elf.segments:
            if loader_segment.loadable:
                break
        else:
            raise Exception("Didn't find loadable segment")

        if loader_segment.virt_addr != self._elf.entry:
            raise Exception("The loader entry point must be the first byte in the image")

        self._image = loader_segment.data

        self._regions: List[Tuple[int, Union[bytes, bytearray]]] = []

        kernel_first_vaddr: Optional[int] = None
        kernel_last_vaddr: Optional[int] = None
        kernel_first_paddr: Optional[int] = None
        kernel_p_v_offset: Optional[int] = None
        for segment in kernel_elf.segments:
            if segment.loadable:
                # NOTE: For now we include any zeroes. We could optimize in the future

                if kernel_first_vaddr is None or segment.virt_addr < kernel_first_vaddr:
                    kernel_first_vaddr = segment.virt_addr

                if kernel_last_vaddr is None or segment.virt_addr + segment.mem_size > kernel_last_vaddr:
                    kernel_last_vaddr = round_up(segment.virt_addr + segment.mem_size, mb(2))

                if kernel_first_paddr is None or segment.phys_addr < kernel_first_paddr:
                    kernel_first_paddr = segment.phys_addr

                if kernel_p_v_offset is None:
                    kernel_p_v_offset = segment.virt_addr - segment.phys_addr
                else:
                    if kernel_p_v_offset != segment.virt_addr - segment.phys_addr:
                        raise Exception("Kernel does not have constistent phys to virt offset")

                self._regions.append((
                    segment.phys_addr,
                    segment.data
                ))



        assert kernel_first_paddr is not None

        # Note: This could be extended to support multi-segment ELF files
        # (and indeed initial did support multi-segment ELF files). However
        # it adds significant complexity, and the calling functions enforce
        # only single-segment ELF files, so we keep things simple here.
        assert len(initial_task_elf.segments) == 1
        segment = initial_task_elf.segments[0]
        assert segment.loadable

        inittask_first_vaddr = segment.virt_addr
        inittask_last_vaddr = round_up(segment.virt_addr + segment.mem_size, kb(4))

        inittask_first_paddr = segment.phys_addr if initial_task_phys_base is None else initial_task_phys_base
        inittask_p_v_offset = inittask_first_vaddr - inittask_first_paddr

        # NOTE: For now we include any zeroes. We could optimize in the future
        self._regions.append((
            inittask_first_paddr,
            segment.data
        ))

        # Determine the pagetable variables
        assert kernel_first_vaddr is not None
        assert kernel_first_paddr is not None
        if kernel_config.hyp_mode:
            pagetable_vars = self._setup_pagetables_hypervisor(kernel_first_vaddr, kernel_first_paddr)
        else:
            pagetable_vars = self._setup_pagetables(kernel_first_vaddr, kernel_first_paddr)
        for var_name, var_data in pagetable_vars.items():
            var_addr, var_size = self._elf.find_symbol(var_name)
            offset = var_addr - loader_segment.virt_addr
            assert var_size == len(var_data)
            assert offset > 0
            assert offset <= len(self._image)
            self._image[offset:offset + var_size] = var_data

        kernel_entry = kernel_elf.entry
        assert inittask_first_paddr is not None
        assert inittask_first_vaddr is not None
        pv_offset = inittask_first_paddr - inittask_first_vaddr

        ui_p_reg_start = inittask_first_paddr
        assert inittask_last_vaddr is not None
        assert inittask_p_v_offset is not None
        ui_p_reg_end = inittask_last_vaddr - inittask_p_v_offset
        assert(ui_p_reg_end > ui_p_reg_start)
        v_entry = initial_task_elf.entry

        extra_device_addr_p = reserved_region.base
        extra_device_size = reserved_region.size

        self._regions += regions

        _check_non_overlapping(self._regions)

        # Currently the only flag passed to the loader is whether seL4
        # is configured as a hypervisor or not.
        flags = 1 if kernel_config.hyp_mode else 0

        self._header = (
            self._magic,
            flags,
            kernel_entry,
            ui_p_reg_start,
            ui_p_reg_end,
            pv_offset,
            v_entry,
            extra_device_addr_p,
            extra_device_size,
            len(self._regions)
        )

    def _setup_pagetables(self, first_vaddr: int, first_paddr: int) -> Dict[str, bytes]:
        boot_lvl1_lower_addr, _ = self._elf.find_symbol("boot_lvl1_lower")
        boot_lvl1_upper_addr, _ = self._elf.find_symbol("boot_lvl1_upper")
        boot_lvl2_upper_addr, _ = self._elf.find_symbol("boot_lvl2_upper")

        boot_lvl0_lower = bytearray(PAGE_TABLE_SIZE)
        boot_lvl0_lower[:8] = pack("<Q", boot_lvl1_lower_addr | 3)

        boot_lvl1_lower = bytearray(PAGE_TABLE_SIZE)
        for i in range(512):
            pt_entry = (
                (i << AARCH64_1GB_BLOCK_BITS) |
                (1 << 10) | # access flag
                (0 << 2) | # strongly ordered memory
                (1) # 1G block
            )
            boot_lvl1_lower[8*i:8*(i+1)] = pack("<Q", pt_entry)

        boot_lvl0_upper = bytearray(PAGE_TABLE_SIZE)
        ptentry = boot_lvl1_upper_addr | 3
        idx = lvl0_index(first_vaddr)
        boot_lvl0_upper[8 * idx:8 * (idx+1)] = pack("<Q", ptentry)

        boot_lvl1_upper = bytearray(PAGE_TABLE_SIZE)
        ptentry = boot_lvl2_upper_addr | 3
        idx = lvl1_index(first_vaddr)
        boot_lvl1_upper[8 * idx:8 * (idx+1)] = pack("<Q", ptentry)

        boot_lvl2_upper = bytearray(PAGE_TABLE_SIZE)
        for i in range(lvl2_index(first_vaddr), 512):
            pt_entry = (
                first_paddr |
                (1 << 10) | # access flag
                (3 << 8) | # make sure the shareability is the same as the kernel's
                (4 << 2) | #MT_NORMAL memory
                (1 << 0) # 2M block
            )
            first_paddr += (1 << AARCH64_2MB_BLOCK_BITS)
            boot_lvl2_upper[8*i:8*(i+1)] = pack("<Q", pt_entry)

        return {
            "boot_lvl0_lower": boot_lvl0_lower,
            "boot_lvl1_lower": boot_lvl1_lower,
            "boot_lvl0_upper": boot_lvl0_upper,
            "boot_lvl1_upper": boot_lvl1_upper,
            "boot_lvl2_upper": boot_lvl2_upper,
        }

    def _setup_pagetables_hypervisor(self, first_vaddr: int, first_paddr: int) -> Dict[str, bytes]:
        boot_lvl1_lower_addr, _ = self._elf.find_symbol("boot_lvl1_lower")
        boot_lvl1_upper_addr, _ = self._elf.find_symbol("boot_lvl1_upper")
        boot_lvl2_upper_addr, _ = self._elf.find_symbol("boot_lvl2_upper")

        boot_lvl0_lower = bytearray(PAGE_TABLE_SIZE)
        boot_lvl0_lower[:8] = pack("<Q", boot_lvl1_lower_addr | 3)

        boot_lvl0_upper = bytearray(PAGE_TABLE_SIZE)

        boot_lvl1_lower = bytearray(PAGE_TABLE_SIZE)
        for i in range(512):
            pt_entry = (
                (i << AARCH64_1GB_BLOCK_BITS) |
                (1 << 10) | # access flag
                (0 << 2) | # strongly ordered memory
                (1) # 1G block
            )
            boot_lvl1_lower[8*i:8*(i+1)] = pack("<Q", pt_entry)

        ptentry = boot_lvl1_upper_addr | 3
        idx = lvl0_index(first_vaddr)
        boot_lvl0_lower[8 * idx:8 * (idx+1)] = pack("<Q", ptentry)

        boot_lvl1_upper = bytearray(PAGE_TABLE_SIZE)
        ptentry = boot_lvl2_upper_addr | 3
        idx = lvl1_index(first_vaddr)
        boot_lvl1_upper[8 * idx:8 * (idx+1)] = pack("<Q", ptentry)

        boot_lvl2_upper = bytearray(PAGE_TABLE_SIZE)
        for i in range(lvl2_index(first_vaddr), 512):
            pt_entry = (
                (((i - lvl2_index(first_vaddr)) << AARCH64_2MB_BLOCK_BITS) + first_paddr) |
                (1 << 10) | # access flag
                (3 << 8) | # make sure the shareability is the same as the kernel's
                (4 << 2) | # MT_NORMAL memory
                (1 << 0) # 2M block
            )
            boot_lvl2_upper[8*i:8*(i+1)] = pack("<Q", pt_entry)

        return {
            "boot_lvl0_lower": boot_lvl0_lower,
            "boot_lvl1_lower": boot_lvl1_lower,
            "boot_lvl0_upper": boot_lvl0_upper,
            "boot_lvl1_upper": boot_lvl1_upper,
            "boot_lvl2_upper": boot_lvl2_upper,
        }

    def write_image(self, path: Path) -> None:
        with path.open("wb") as f:
            header_binary = pack(self._header_struct_fmt, *self._header)
            offset = 0
            for addr, data in self._regions:
                header_binary += pack(self._region_struct_fmt, addr, len(data), offset, 1)
                offset += len(data)

            # Finally write everything out to a file.
            f.write(self._image)
            f.write(header_binary)
            for _, data in self._regions:
                f.write(data)
