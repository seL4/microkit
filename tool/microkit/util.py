#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
from dataclasses import dataclass
from typing import List, Optional

class UserError(Exception):
    pass


def kb(n: int) -> int:
    return n * 1024


def mb(n: int) -> int:
    return n * 1024 * 1024


def msb(x: int) -> int:
    return x.bit_length() - 1


def lsb(x: int) -> int:
    return msb(x & -x)


def round_up(n: int, x: int) -> int:
    d, m = divmod(n, x)
    return n if m == 0 else n + x - m


def round_down(n: int, x: int) -> int:
    d, m = divmod(n, x)
    return n if m == 0 else n - m

def mask_bits(n: int, bits: int) -> int:
    """mask out (set to zero) the lower bits from n"""
    assert n > 0
    return (n >> bits) << bits


def is_power_of_two(n: int) -> bool:
    """Return True if n is a power of two."""
    assert n > 0
    return n & (n - 1) == 0


def str_to_bool(s: str) -> bool:
    if s.lower() == "true":
        return True
    elif s.lower() == "false":
        return False
    raise ValueError("invalid boolean value")

def machine_wrap(n: int) -> int:
    # Note that we assume a 64-bit word size.
    # @ivanv: maybe a bad assumption
    return n & ((2 ** 64) - 1)

def machine_add(a: int, b: int) -> int:
    return machine_wrap(a + b)

def machine_sub(a: int, b: int) -> int:
    return machine_wrap(a - b)

def paddr_to_kernel_vaddr(kernel_virtual_base: int, paddr: int) -> int:
    return machine_add(paddr, kernel_virtual_base)

def kernel_vaddr_to_paddr(kernel_virtual_base: int, kernel_vaddr: int) -> int:
    return machine_sub(kernel_vaddr, kernel_virtual_base)


@dataclass
class MemoryRegion:
    # Note: base is inclusive, end is exclusive
    # MemoryRegion(1, 5) would have a size of 4
    # and cover [1, 2, 3, 4]
    base: int
    end: int

    def aligned_power_of_two_regions(self, kernel_virtual_base: int, max_bits: int) -> List["MemoryRegion"]:
        # During the boot phase, the kernel creates all of the untyped regions
        # based on the kernel virtual addresses, rather than the physical
        # memory addresses. This has a subtle side affect in the process of
        # creating untypeds as even though all the kernel virtual addresses are
        # a constant offest of the corresponding physical address, overflow can
        # occur when dealing with virtual addresses. This precisely occurs in
        # this function, causing different regions depending on whether
        # you use kernel virtual or physical addresses. In order to properly
        # emulate the kernel booting process, we also have to emulate the interger
        # overflow that can occur.
        r = []
        base = paddr_to_kernel_vaddr(kernel_virtual_base, self.base)
        end = paddr_to_kernel_vaddr(kernel_virtual_base, self.end)
        while base != end:
            size = machine_sub(end, base)
            size_bits = msb(size)
            if base == 0:
                bits = size_bits
            else:
                bits = min(size_bits, lsb(base))

            if bits > max_bits:
                bits = max_bits
            sz = 1 << bits
            base_paddr = kernel_vaddr_to_paddr(kernel_virtual_base, base)
            end_paddr = kernel_vaddr_to_paddr(kernel_virtual_base, base + sz)
            base = machine_add(base, sz)
            r.append(MemoryRegion(base_paddr, end_paddr))

        return r

    def __repr__(self) -> str:
        return f"MemoryRegion(base=0x{self.base:x}, end=0x{self.end:x})"

    @property
    def size(self) -> int:
        return self.end - self.base


class DisjointMemoryRegion:
    def __init__(self) -> None:
        self._regions: List[MemoryRegion] = []
        self._check()

    def _check(self) -> None:
        # Ensure that regions are sorted and non-overlapping
        last_end: Optional[int] = None
        for region in self._regions:
            if last_end is not None:
                assert region.base >= last_end
            last_end = region.end

    def dump(self) -> None:
        for region in self._regions:
            print(f"   {region.base:016x} - {region.end:016x}")

    def insert_region(self, base: int, end: int) -> None:
        # Find where it belongs
        for idx, region in enumerate(self._regions):
            if end < region.base:
                break
        else:
            idx = len(self._regions)
        # FIXME: Should extend here if adjacent rather than
        # inserting now
        self._regions.insert(idx, MemoryRegion(base, end))
        self._check()

    def remove_region(self, base: int, end: int) -> None:
        for idx, region in enumerate(self._regions):
            if base >= region.base and end <= region.end:
                break
        else:
            raise ValueError(f"Attempting to remove region (0x{base:x}-0x{end:x}) that is not currently covered")

        if region.base == base and region.end == end:
            # Covers exactly, so just remove
            del self._regions[idx]
        elif region.base == base:
            # Trim the start of the region
            self._regions[idx] = MemoryRegion(end, region.end)
        elif region.end == end:
            # Trim end of the region
            self._regions[idx] = MemoryRegion(region.base, base)
        else:
            # Splitting
            self._regions[idx] = MemoryRegion(region.base, base)
            self._regions.insert(idx + 1, MemoryRegion(end, region.end))

        self._check()

    def aligned_power_of_two_regions(self, kernel_virtual_base: int, max_bits: int) -> List[MemoryRegion]:
        r = []
        for region in self._regions:
            r += region.aligned_power_of_two_regions(kernel_virtual_base, max_bits)
        return r

    def allocate(self, size: int) -> int:
        """Allocate region of 'size' bytes, returning the base address.

        The allocated region is removed from the disjoint memory region."""

        # Allocation policy is simple first fit.
        # Possibly a 'best fit' policy would be better.
        # 'best' may be something that best matches a power-of-two
        # allocation
        for region in self._regions:
            if size <= region.size:
                break
        else:
            raise ValueError(f"Unable to allocate 0x{size:x} bytes.")

        self.remove_region(region.base, region.base + size)

        return region.base

    def allocate_from(self, size: int, lower_bound: int) -> int:
        for region in self._regions:
            if size <= region.size and region.base >= lower_bound:
                break
        else:
            raise ValueError(f"Unable to allocate 0x{size:x} bytes.")

        self.remove_region(region.base, region.base + size)

        return region.base

    def __repr__(self) -> str:
        return " ".join(map(lambda x: "0x%x->0x%x" %(x.base, x.base + x.size), self._regions))
