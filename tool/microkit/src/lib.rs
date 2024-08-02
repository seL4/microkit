//
// Copyright 2024, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

pub mod elf;
pub mod loader;
pub mod sdf;
pub mod sel4;
pub mod util;

use sel4::BootInfo;
use std::cmp::min;
use std::fmt;

// Note that this value is used in the monitor so should also be changed there
// if this was to change.
pub const MAX_PDS: usize = 63;
// It should be noted that if you were to change the value of
// the maximum PD name length, you would also have to change
// the monitor and libmicrokit.
pub const PD_MAX_NAME_LENGTH: usize = 16;

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct UntypedObject {
    pub cap: u64,
    pub region: MemoryRegion,
    pub is_device: bool,
}

impl UntypedObject {
    pub fn new(cap: u64, region: MemoryRegion, is_device: bool) -> UntypedObject {
        UntypedObject {
            cap,
            region,
            is_device,
        }
    }

    pub fn base(&self) -> u64 {
        self.region.base
    }

    pub fn end(&self) -> u64 {
        self.region.end
    }

    pub fn size_bits(&self) -> u64 {
        util::lsb(self.region.size())
    }
}

pub struct Region {
    pub name: String,
    pub addr: u64,
    pub size: u64,
    // In order to avoid some expensive copies to put the data
    // into this struct, we instead store the index of the segment
    // of the ELF this region is associated with.
    segment_idx: usize,
}

impl Region {
    pub fn new(name: String, addr: u64, size: u64, segment_idx: usize) -> Region {
        Region {
            name,
            addr,
            size,
            segment_idx,
        }
    }

    pub fn data<'a>(&self, elf: &'a elf::ElfFile) -> &'a Vec<u8> {
        &elf.segments[self.segment_idx].data
    }
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "<Region name={} addr=0x{:x} size={}>",
            self.name, self.addr, self.size
        )
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MemoryRegion {
    /// Note: base is inclusive, end is exclusive
    /// MemoryRegion(1, 5) would have a size of 4
    /// and cover [1, 2, 3, 4]
    pub base: u64,
    pub end: u64,
}

impl fmt::Display for MemoryRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MemoryRegion(base=0x{:x}, end=0x{:x})",
            self.base, self.end
        )
    }
}

impl MemoryRegion {
    pub fn new(base: u64, end: u64) -> MemoryRegion {
        MemoryRegion { base, end }
    }

    pub fn size(&self) -> u64 {
        self.end - self.base
    }

    pub fn aligned_power_of_two_regions(&self, max_bits: u64) -> Vec<MemoryRegion> {
        let mut regions = Vec::new();
        let mut base = self.base;
        let mut bits;
        while base != self.end {
            let size = self.end - base;
            let size_bits = util::msb(size);
            if base == 0 {
                bits = size_bits;
            } else {
                bits = min(size_bits, util::lsb(base));
            }

            if bits > max_bits {
                bits = max_bits;
            }
            let sz = 1 << bits;
            regions.push(MemoryRegion::new(base, base + sz));
            base += sz;
        }

        regions
    }
}

#[derive(Default)]
pub struct DisjointMemoryRegion {
    pub regions: Vec<MemoryRegion>,
}

impl DisjointMemoryRegion {
    fn check(&self) {
        // Ensure that regions are sorted and non-overlapping
        let mut last_end: Option<u64> = None;
        for region in &self.regions {
            if last_end.is_some() {
                assert!(region.base >= last_end.unwrap());
            }
            last_end = Some(region.end)
        }
    }

    pub fn insert_region(&mut self, base: u64, end: u64) {
        let mut insert_idx = self.regions.len();
        for (idx, region) in self.regions.iter().enumerate() {
            if end <= region.base {
                insert_idx = idx;
                break;
            }
        }
        // FIXME: Should extend here if adjacent rather than
        // inserting now
        self.regions
            .insert(insert_idx, MemoryRegion::new(base, end));
        self.check();
    }

    pub fn remove_region(&mut self, base: u64, end: u64) {
        let mut maybe_idx = None;
        for (i, r) in self.regions.iter().enumerate() {
            if base >= r.base && end <= r.end {
                maybe_idx = Some(i);
                break;
            }
        }
        if maybe_idx.is_none() {
            panic!("Internal error: attempting to remove region [0x{:x}-0x{:x}) that is not currently covered", base, end);
        }

        let idx = maybe_idx.unwrap();

        let region = self.regions[idx];

        if region.base == base && region.end == end {
            // Covers exactly, so just remove
            self.regions.remove(idx);
        } else if region.base == base {
            // Trim the start of the region
            self.regions[idx] = MemoryRegion::new(end, region.end);
        } else if region.end == end {
            // Trim end of the region
            self.regions[idx] = MemoryRegion::new(region.base, base);
        } else {
            // Splitting
            self.regions[idx] = MemoryRegion::new(region.base, base);
            self.regions
                .insert(idx + 1, MemoryRegion::new(end, region.end));
        }

        self.check();
    }

    pub fn aligned_power_of_two_regions(&self, max_bits: u64) -> Vec<MemoryRegion> {
        let mut aligned_regions = Vec::new();
        for region in &self.regions {
            aligned_regions.extend(region.aligned_power_of_two_regions(max_bits));
        }

        aligned_regions
    }

    /// Allocate region of 'size' bytes, returning the base address.
    /// The allocated region is removed from the disjoint memory region.
    /// Allocation policy is simple first fit.
    /// Possibly a 'best fit' policy would be better.
    /// 'best' may be something that best matches a power-of-two
    /// allocation
    pub fn allocate(&mut self, size: u64) -> u64 {
        let mut region_to_remove: Option<MemoryRegion> = None;
        for region in &self.regions {
            if size <= region.size() {
                region_to_remove = Some(*region);
                break;
            }
        }

        match region_to_remove {
            Some(region) => {
                self.remove_region(region.base, region.base + size);
                region.base
            }
            None => panic!("Unable to allocate {} bytes", size),
        }
    }

    pub fn allocate_from(&mut self, size: u64, lower_bound: u64) -> u64 {
        let mut region_to_remove = None;
        for region in &self.regions {
            if size <= region.size() && region.base >= lower_bound {
                region_to_remove = Some(*region);
                break;
            }
        }

        match region_to_remove {
            Some(region) => {
                self.remove_region(region.base, region.base + size);
                region.base
            }
            None => panic!(
                "Unable to allocate {} bytes from lower_bound 0x{:x}",
                size, lower_bound
            ),
        }
    }
}

#[derive(Copy, Clone)]
pub struct KernelAllocation {
    pub untyped_cap_address: u64, // FIXME: possibly this is an object, not an int?
    pub phys_addr: u64,
}

pub struct UntypedAllocator {
    untyped_object: UntypedObject,
    allocation_point: u64,
    allocations: Vec<KernelAllocation>,
}

impl UntypedAllocator {
    pub fn new(
        untyped_object: UntypedObject,
        allocation_point: u64,
        allocations: Vec<KernelAllocation>,
    ) -> UntypedAllocator {
        UntypedAllocator {
            untyped_object,
            allocation_point,
            allocations,
        }
    }

    pub fn base(&self) -> u64 {
        self.untyped_object.region.base
    }

    pub fn end(&self) -> u64 {
        self.untyped_object.region.end
    }
}

/// Allocator for kernel objects.
///
/// This tracks the space available in a set of untyped objects.
/// On allocation an untyped with sufficient remaining space is
/// returned (while updating the internal tracking).
///
/// Within an untyped object this mimics the kernel's allocation
/// policy (basically a bump allocator with alignment).
///
/// The only 'choice' this allocator has is which untyped object
/// to use. The current algorithm is simply first fit: the first
/// untyped that has sufficient space. This is not optimal.
///
/// Note: The allocator does not generate the Retype invocations;
/// this must be done with more knowledge (specifically the destination
/// cap) which is distinct.
///
/// It is critical that invocations are generated in the same order
/// as the allocations are made.
pub struct ObjectAllocator {
    allocation_idx: u64,
    untyped: Vec<UntypedAllocator>,
}

impl ObjectAllocator {
    pub fn new(kernel_boot_info: &BootInfo) -> ObjectAllocator {
        let mut untyped = Vec::new();
        for ut in kernel_boot_info.untyped_objects.iter() {
            if ut.is_device {
                // Kernel allocator can only allocate out of normal memory
                // device memory can't be used for kernel objects
                continue;
            }
            untyped.push(UntypedAllocator::new(*ut, 0, vec![]));
        }

        ObjectAllocator {
            allocation_idx: 0,
            untyped,
        }
    }

    pub fn alloc(&mut self, size: u64) -> KernelAllocation {
        self.alloc_n(size, 1)
    }

    pub fn alloc_n(&mut self, size: u64, count: u64) -> KernelAllocation {
        assert!(util::is_power_of_two(size));
        assert!(count > 0);
        for ut in &mut self.untyped {
            // See if this fits
            let start = util::round_up(ut.base() + ut.allocation_point, size);
            if start + (count * size) <= ut.end() {
                ut.allocation_point = (start - ut.base()) + (count * size);
                self.allocation_idx += 1;
                let allocation = KernelAllocation {
                    untyped_cap_address: ut.untyped_object.cap,
                    phys_addr: start,
                };
                ut.allocations.push(allocation);
                return allocation;
            }
        }

        panic!("Can't alloc of size {}, count: {} - no space", size, count);
    }
}
