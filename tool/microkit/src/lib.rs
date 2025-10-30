//
// Copyright 2024, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

use std::{cmp::min, fmt};

use crate::{sel4::Config, util::struct_to_bytes};

pub mod capdl;
pub mod elf;
pub mod loader;
pub mod report;
pub mod sdf;
pub mod sel4;
pub mod symbols;
pub mod util;

// Note that these values are used in the monitor so should also be changed there
// if any of these were to change.
pub const MAX_PDS: usize = 63;
pub const MAX_VMS: usize = 63;
// It should be noted that if you were to change the value of
// the maximum PD/VM name length, you would also have to change
// the monitor and libmicrokit.
pub const PD_MAX_NAME_LENGTH: usize = 64;
pub const VM_MAX_NAME_LENGTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct UntypedObject {
    pub cap: u64,
    pub region: MemoryRegion,
    pub is_device: bool,
}

pub const UNTYPED_DESC_PADDING: usize = size_of::<u64>() - (2 * size_of::<u8>());
#[repr(C)]
struct SeL4UntypedDesc {
    paddr: u64,
    size_bits: u8,
    is_device: u8,
    padding: [u8; UNTYPED_DESC_PADDING],
}

impl From<&UntypedObject> for SeL4UntypedDesc {
    fn from(value: &UntypedObject) -> Self {
        Self {
            paddr: value.base(),
            size_bits: value.size_bits() as u8,
            is_device: if value.is_device { 1 } else { 0 },
            padding: [0u8; UNTYPED_DESC_PADDING],
        }
    }
}

/// Getting a `seL4_UntypedDesc` for patching into the initialiser
pub fn serialise_ut(ut: &UntypedObject) -> Vec<u8> {
    let sel4_untyped_desc: SeL4UntypedDesc = ut.into();
    unsafe { struct_to_bytes(&sel4_untyped_desc).to_vec() }
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
        elf.segments[self.segment_idx].data()
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

    pub fn aligned_power_of_two_regions(
        &self,
        config: &Config,
        max_bits: u64,
    ) -> Vec<MemoryRegion> {
        // During the boot phase, the kernel creates all of the untyped regions
        // based on the kernel virtual addresses, rather than the physical
        // memory addresses. This has a subtle side affect in the process of
        // creating untypeds as even though all the kernel virtual addresses are
        // a constant offset of the corresponding physical address, overflow can
        // occur when dealing with virtual addresses. This precisely occurs in
        // this function, causing different regions depending on whether
        // you use kernel virtual or physical addresses. In order to properly
        // emulate the kernel booting process, we also have to emulate the unsigned integer
        // overflow that can occur.
        let mut regions = Vec::new();
        let mut base = config.paddr_to_kernel_vaddr(self.base);
        let end = config.paddr_to_kernel_vaddr(self.end);
        let mut bits;
        while base != end {
            let size = end.wrapping_sub(base);
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
            let base_paddr = config.kernel_vaddr_to_paddr(base);
            let end_paddr = config.kernel_vaddr_to_paddr(base.wrapping_add(sz));
            regions.push(MemoryRegion::new(base_paddr, end_paddr));
            base = base.wrapping_add(sz);
        }

        regions
    }
}

#[derive(Default, Debug)]
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
            panic!("Internal error: attempting to remove region [0x{base:x}-0x{end:x}) that is not currently covered");
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

    pub fn aligned_power_of_two_regions(
        &self,
        config: &Config,
        max_bits: u64,
    ) -> Vec<MemoryRegion> {
        let mut aligned_regions = Vec::new();
        for region in &self.regions {
            aligned_regions.extend(region.aligned_power_of_two_regions(config, max_bits));
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
            None => panic!("Unable to allocate {size} bytes"),
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
            None => panic!("Unable to allocate {size} bytes from lower_bound 0x{lower_bound:x}"),
        }
    }
}
