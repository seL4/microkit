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

use sel4::Config;
use std::cmp::min;
use std::fmt;

use crate::sel4::PageSize;

// Note that these values are used in the monitor so should also be changed there
// if any of these were to change.
pub const MAX_PDS: usize = 63;
pub const MAX_VMS: usize = 63;
// It should be noted that if you were to change the value of
// the maximum PD/VM name length, you would also have to change
// the monitor and libmicrokit.
pub const PD_MAX_NAME_LENGTH: usize = 64;
pub const VM_MAX_NAME_LENGTH: usize = 64;

// Note that these constants align with the only architectures that we are
// supporting at the moment
pub const PAGE_TABLE_ENTRIES: u64 = 512;
pub const PAGE_TABLE_MASK: u64 = 0x1ff;
pub enum PageTableMaskShift {
    PGD = 39,
    PUD = 30,
    PD = 21,
    PT = 12,
}


#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PGD {
    puds: Vec<Option<PUD>>,
}

impl Default for PGD {
    fn default() -> Self {
        Self::new()
    }
}

impl PGD {
    pub fn new() -> Self {
        PGD {
            puds: vec![None; PAGE_TABLE_ENTRIES as usize],
        }
    }

    pub fn recurse(&mut self, mut curr_offset: u64, buffer: &mut Vec<u8>) -> u64 {
        let mut offset_table: [u64; PAGE_TABLE_ENTRIES as usize] = [u64::MAX; PAGE_TABLE_ENTRIES as usize];
        for (i, entry) in offset_table.iter_mut().enumerate() {
            if let Some(pud) = &mut self.puds[i] {
                curr_offset = pud.recurse(curr_offset, buffer);
                *entry = curr_offset - (PAGE_TABLE_ENTRIES * 8);
            }
        }

        for value in &mut offset_table {
            buffer.append(&mut value.to_le_bytes().to_vec());
        }
        curr_offset + (PAGE_TABLE_ENTRIES * 8)
    }

    pub fn add_page_at_vaddr(&mut self, vaddr: u64, frame: u64, size: PageSize) {
        let pgd_index = ((vaddr & (PAGE_TABLE_MASK << PageTableMaskShift::PGD as u64)) >> PageTableMaskShift::PGD as u64) as usize;
        if self.puds[pgd_index].is_none() {
            self.puds[pgd_index] = Some(PUD::new());
        }
        self.puds[pgd_index]
            .as_mut()
            .unwrap()
            .add_page_at_vaddr(vaddr, frame, size);
    }

    pub fn add_page_at_vaddr_range(
        &mut self,
        mut vaddr: u64,
        mut data_len: i64,
        frame: u64,
        size: PageSize,
    ) {
        while data_len > 0 {
            self.add_page_at_vaddr(vaddr, frame, size);
            data_len -= size as i64;
            vaddr += size as u64;
        }
    }

    pub fn get_size(&self) -> u64 {
        let mut child_size = 0;
        for pud in &self.puds {
            if pud.is_some() {
                child_size += pud.as_ref().unwrap().get_size();
            }
        }
        (PAGE_TABLE_ENTRIES * 8) + child_size
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PUD {
    dirs: Vec<Option<DIR>>,
}

impl PUD {
    pub fn new() -> Self {
        PUD {
            dirs: vec![None; PAGE_TABLE_ENTRIES as usize],
        }
    }

    pub fn recurse(&mut self, mut curr_offset: u64, buffer: &mut Vec<u8>) -> u64 {
        let mut offset_table: [u64; PAGE_TABLE_ENTRIES as usize] = [u64::MAX; PAGE_TABLE_ENTRIES as usize];
        for (i, entry) in offset_table.iter_mut().enumerate() {
            if let Some(dir) = &mut self.dirs[i] {
                curr_offset = dir.recurse(curr_offset, buffer);
                *entry = curr_offset - (PAGE_TABLE_ENTRIES * 8);
            }
        }

        for value in &mut offset_table {
            buffer.append(&mut value.to_le_bytes().to_vec());
        }
        curr_offset + (PAGE_TABLE_ENTRIES * 8)
    }

    pub fn add_page_at_vaddr(&mut self, vaddr: u64, frame: u64, size: PageSize) {
        let pud_index = ((vaddr & (PAGE_TABLE_MASK << PageTableMaskShift::PUD as u64)) >> PageTableMaskShift::PUD as u64) as usize;
        if self.dirs[pud_index].is_none() {
            self.dirs[pud_index] = Some(DIR::new());
        }
        self.dirs[pud_index]
            .as_mut()
            .unwrap()
            .add_page_at_vaddr(vaddr, frame, size);
    }

    pub fn add_page_at_vaddr_range(
        &mut self,
        mut vaddr: u64,
        mut data_len: i64,
        frame: u64,
        size: PageSize,
    ) {
        while data_len > 0 {
            self.add_page_at_vaddr(vaddr, frame, size);
            data_len -= size as i64;
            vaddr += size as u64;
        }
    }

    pub fn get_size(&self) -> u64 {
        let mut child_size = 0;
        for dir in &self.dirs {
            if dir.is_some() {
                child_size += dir.as_ref().unwrap().get_size();
            }
        }
        (PAGE_TABLE_ENTRIES * 8) + child_size
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DirEntry {
    PageTable(PT),
    LargePage(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DIR {
    entries: Vec<Option<DirEntry>>,
}

impl DIR {
    fn new() -> Self {
        DIR {
            entries: vec![None; PAGE_TABLE_ENTRIES as usize],
        }
    }

    fn recurse(&mut self, mut curr_offset: u64, buffer: &mut Vec<u8>) -> u64 {
        let mut offset_table: [u64; PAGE_TABLE_ENTRIES as usize] = [u64::MAX; PAGE_TABLE_ENTRIES as usize];
        for (i, dir_entry) in offset_table.iter_mut().enumerate() {
            if let Some(entry) = &mut self.entries[i] {
                match entry {
                    DirEntry::PageTable(x) => {
                        curr_offset = x.recurse(curr_offset, buffer);
                        *dir_entry = curr_offset - (PAGE_TABLE_ENTRIES * 8);
                    }
                    DirEntry::LargePage(x) => {
                        // we mark the top bit to signal to the pd that this is a large page
                        *dir_entry = *x | (1 << 63);
                    }
                }
            }
        }

        for value in &mut offset_table {
            buffer.append(&mut value.to_le_bytes().to_vec());
        }
        curr_offset + (PAGE_TABLE_ENTRIES * 8)
    }

    fn add_page_at_vaddr(&mut self, vaddr: u64, frame: u64, size: PageSize) {
        let dir_index = ((vaddr & (PAGE_TABLE_MASK << PageTableMaskShift::PD as u64)) >> PageTableMaskShift::PD as u64) as usize;
        match size {
            PageSize::Small => {
                if self.entries[dir_index].is_none() {
                    self.entries[dir_index] = Some(DirEntry::PageTable(PT::new()));
                }
                match &mut self.entries[dir_index] {
                    Some(DirEntry::PageTable(x)) => {
                        x.add_page_at_vaddr(vaddr, frame, size);
                    }
                    _ => {
                        panic!("Trying to add small page where a large page already exists!");
                    }
                }
            }
            PageSize::Large => {
                if let Some(DirEntry::PageTable(_)) = self.entries[dir_index] {
                    panic!("Attempting to insert a large page where a page table already exists!");
                }
                self.entries[dir_index] = Some(DirEntry::LargePage(frame));
            }
        }
    }

    fn get_size(&self) -> u64 {
        let mut child_size = 0;
        for pt in &self.entries {
            if let Some(DirEntry::PageTable(x)) = pt {
                child_size += x.get_size();
            }
        }
        (PAGE_TABLE_ENTRIES * 8) + child_size
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PT {
    large_page: u64,
    pages: Vec<u64>,
}

impl PT {
    fn new() -> Self {
        PT {
            pages: vec![u64::MAX; PAGE_TABLE_ENTRIES as usize],
            large_page: u64::MAX,
        }
    }

    fn recurse(&mut self, curr_offset: u64, buffer: &mut Vec<u8>) -> u64 {
        for value in &mut self.pages {
            buffer.append(&mut value.to_le_bytes().to_vec());
        }
        curr_offset + (PAGE_TABLE_ENTRIES * 8)
    }

    fn add_page_at_vaddr(&mut self, vaddr: u64, frame: u64, size: PageSize) {
        let pt_index = ((vaddr & (PAGE_TABLE_MASK << PageTableMaskShift::PT as u64)) >> PageTableMaskShift::PT as u64) as usize;
        // Unconditionally overwrite.
        assert!(size == PageSize::Small);
        self.pages[pt_index] = frame;
    }

    fn get_size(&self) -> u64 {
        PAGE_TABLE_ENTRIES * 8
    }
}

#[derive(Debug, Clone)]
pub enum TopLevelPageTable {
    Riscv64 { top_level: PUD },
    Aarch64 { top_level: PGD},
}

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
    pub size: u64,
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
    pub init_capacity: u64,
    allocation_idx: u64,
    pub untyped: Vec<UntypedAllocator>,
}

/// First entry is potential padding, then the actual allocation is the second
/// entry.
type FixedAllocation = (Option<Vec<KernelAllocation>>, KernelAllocation);

pub enum FindFixedError {
    AlreadyAllocated,
    TooLarge,
}

impl ObjectAllocator {
    pub fn new(untyped_pool: Vec<&UntypedObject>) -> ObjectAllocator {
        let mut untyped: Vec<UntypedAllocator> = untyped_pool
            .into_iter()
            .map(|ut| UntypedAllocator::new(*ut, 0, vec![]))
            .collect();
        untyped.sort_by(|a, b| a.untyped_object.base().cmp(&b.untyped_object.base()));

        let mut capacity = 0;
        for ut in &untyped {
            capacity += ut.end() - (ut.base() + ut.allocation_point);
        }

        ObjectAllocator {
            init_capacity: capacity,
            allocation_idx: 0,
            untyped,
        }
    }

    pub fn capacity(&self) -> u64 {
        let mut capacity = 0;
        for ut in &self.untyped {
            capacity += ut.end() - (ut.base() + ut.allocation_point);
        }

        capacity
    }

    pub fn max_alloc_size(&self) -> u64 {
        let mut largest_capacity = 0;
        for ut in &self.untyped {
            let ut_capacity = ut.end() - (ut.base() + ut.allocation_point);
            if ut_capacity > largest_capacity {
                largest_capacity = ut_capacity;
            }
        }

        largest_capacity
    }

    pub fn alloc(&mut self, size: u64) -> Option<KernelAllocation> {
        self.alloc_n(size, 1)
    }

    pub fn alloc_n(&mut self, size: u64, count: u64) -> Option<KernelAllocation> {
        assert!(util::is_power_of_two(size));
        assert!(count > 0);
        let mem_size = count * size;
        for ut in &mut self.untyped {
            // See if this fits
            let start = util::round_up(ut.base() + ut.allocation_point, size);
            if start + mem_size <= ut.end() {
                ut.allocation_point = (start - ut.base()) + mem_size;
                self.allocation_idx += 1;
                let allocation = KernelAllocation {
                    untyped_cap_address: ut.untyped_object.cap,
                    phys_addr: start,
                    size: mem_size,
                };
                ut.allocations.push(allocation);
                return Some(allocation);
            }
        }

        None
    }

    pub fn reserve(&mut self, alloc: (&UntypedObject, u64)) {
        for ut in &mut self.untyped {
            if *alloc.0 == ut.untyped_object {
                if ut.base() <= alloc.1 && alloc.1 <= ut.end() {
                    ut.allocation_point = alloc.1 - ut.base();
                    return;
                } else {
                    panic!(
                        "Allocation {:?} ({:x}) not in untyped region {:?}",
                        alloc.0, alloc.1, ut.untyped_object
                    );
                }
            }
        }

        panic!(
            "Allocation {:?} ({:x}) not in any device untyped",
            alloc.0, alloc.1
        );
    }

    pub fn find_fixed(
        &mut self,
        phys_addr: u64,
        size: u64,
    ) -> Result<Option<FixedAllocation>, FindFixedError> {
        for ut in &mut self.untyped {
            /* Find the right untyped */
            if phys_addr >= ut.base() && phys_addr < ut.end() {
                if phys_addr < ut.base() + ut.allocation_point {
                    return Err(FindFixedError::AlreadyAllocated);
                }

                let space_left = ut.end() - (ut.base() + ut.allocation_point);
                if space_left < size {
                    return Err(FindFixedError::TooLarge);
                }

                let mut watermark = ut.base() + ut.allocation_point;
                let mut allocations: Option<Vec<KernelAllocation>>;

                if phys_addr != watermark {
                    allocations = Some(Vec::new());
                    /* If the watermark isn't at the right place, we need to pad */
                    let mut padding_required = phys_addr - watermark;
                    // We are restricted in how much we can pad:
                    // 1: Untyped objects must be power-of-two sized.
                    // 2: Untyped objects must be aligned to their size.
                    let mut padding_sizes = Vec::new();
                    // We have two potential approaches for how we pad.
                    // 1: Use largest objects possible respecting alignment
                    // and size restrictions.
                    // 2: Use a fixed size object multiple times. This will
                    // create more objects, but as same sized objects can be
                    // create in a batch, required fewer invocations.
                    // For now we choose #1
                    while padding_required > 0 {
                        let wm_lsb = util::lsb(watermark);
                        let sz_msb = util::msb(padding_required);
                        let pad_object_size = 1 << min(wm_lsb, sz_msb);
                        padding_sizes.push(pad_object_size);

                        allocations.as_mut().unwrap().push(KernelAllocation {
                            untyped_cap_address: ut.untyped_object.cap,
                            phys_addr: watermark,
                            size: pad_object_size,
                        });

                        watermark += pad_object_size;
                        padding_required -= pad_object_size;
                    }
                } else {
                    allocations = None;
                }

                let obj = KernelAllocation {
                    untyped_cap_address: ut.untyped_object.cap,
                    phys_addr: watermark,
                    size,
                };

                ut.allocation_point = (watermark + size) - ut.base();
                return Ok(Some((allocations, obj)));
            }
        }

        Ok(None)
    }
}
