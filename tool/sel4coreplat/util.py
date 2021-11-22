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



@dataclass
class MemoryRegion:
    # Note: base is inclusive, end is exclusive
    # MemoryRegion(1, 5) would have a size of 4
    # and cover [1, 2, 3, 4]
    base: int
    end: int

    def aligned_power_of_two_regions(self) -> List["MemoryRegion"]:
        max_bits = 40
        # Align
        # find the first bit self
        r = []
        base = self.base
        end = self.end
        while base != end:
            size = end - base
            size_bits = msb(size)
            if base == 0:
                bits = size_bits
            else:
                bits = min(size_bits, lsb(base))

            if bits > max_bits:
                bits = max_bits
            sz = 1 << bits
            r.append(MemoryRegion(base, base + sz))
            base += sz

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

    def aligned_power_of_two_regions(self) -> List[MemoryRegion]:
        r = []
        for region in self._regions:
            r += region.aligned_power_of_two_regions()
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
            raise ValueError(f"Unable to allocate {size} bytes.")

        self.remove_region(region.base, region.base + size)

        return region.base
