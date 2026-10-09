//
// Copyright 2026, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

#![no_std]
// We prefer indices as it matches the semantics of PT indices
#![allow(clippy::needless_range_loop)]

mod aligned_regions;
mod c_interop;
mod shared_types;

use core::mem;
use core::mem::MaybeUninit;
use core::slice;

use aligned_regions::AlignedRegionsIter;
use shared_types::{AArch64ReturnValue, MmuRegion as Region};

const PAGE_TABLE_SIZE: usize = 4096;

const fn divmod(x: u64, y: u64) -> (u64, u64) {
    (x / y, x % y)
}

const fn mask(n: u64) -> u64 {
    (1 << n) - 1
}

const fn round_down(n: u64, x: u64) -> u64 {
    let (_, m) = divmod(n, x);
    if m == 0 {
        n
    } else {
        n - m
    }
}

const fn align_down(n: u64, bits: u64) -> u64 {
    round_down(n, 1 << bits)
}

pub mod aarch64 {
    //! For AArch64, our page tables use the Stage 1 descriptor formats
    //! for both EL2 (TTBR0_EL2) and EL1 (TTBR0_EL1/TTBR1_EL1).
    //! Stage 2 descriptors are only used when in the EL1&0 regime; which is not
    //! the case when in EL2.

    use super::*;

    pub const LVL0_BITS: u64 = 9;
    pub const LVL1_BITS: u64 = 9;
    pub const LVL2_BITS: u64 = 9;
    pub const LVL3_BITS: u64 = 9;

    pub fn lvl0_index(addr: u64) -> usize {
        let idx = (addr >> (BLOCK_BITS_2MB + LVL2_BITS + LVL1_BITS)) & mask(LVL0_BITS);
        idx as usize
    }

    pub fn lvl1_index(addr: u64) -> usize {
        let idx = (addr >> (BLOCK_BITS_2MB + LVL2_BITS)) & mask(LVL1_BITS);
        idx as usize
    }

    pub fn lvl2_index(addr: u64) -> usize {
        let idx = (addr >> (BLOCK_BITS_2MB)) & mask(LVL2_BITS);
        idx as usize
    }

    pub fn lvl3_index(addr: u64) -> usize {
        let idx = (addr >> PAGE_BITS_4KB) & mask(LVL3_BITS);
        idx as usize
    }

    /// Stage 1 translation table page/block descriptors have bits[4:2] containing
    /// AttrIndex[2:0]. The AttrIndex values depends on our configuration of
    /// the `MAIR_EL1` or `MAIR_EL2` registers done in util64.S;
    /// This also needs to match the values that seL4 uses.
    #[allow(non_upper_case_globals, reason = "matching ARM naming convention")]
    pub mod s1_mair_attr_index {
        pub const MT_DEVICE_nGnRnE: u64 = 0b000;
        pub const MT_DEVICE_nGnRE: u64 = 0b001;
        pub const MT_DEVICE_GRE: u64 = 0b010;
        pub const MT_NORMAL_NC: u64 = 0b011;
        pub const MT_NORMAL: u64 = 0b100;
    }

    pub mod descriptor_type {
        //! The translation table descriptor formats, as per §D8.3 "Translation
        //! table descriptor formats" of ARM DDI 0487 L.b. Specifically,
        //! as per "Table D8-48 Determination of descriptor type"

        /// Descriptor type: Table. Condition is lookup level != 3.
        pub const TABLE: u64 = 0b11;
        /// Descriptor type: Page. Condition is lookup level == 3.
        pub const PAGE: u64 = 0b11;
        /// Descriptor type: Block. Condition is lookup level != 3.
        pub const BLOCK: u64 = 0b01;
        /// Descriptor type: Invalid. Strictly speaking bit[1] does not matter.
        pub const INVALID: u64 = 0b00;
    }

    pub mod shareability_attributes {
        //! Per §D8.6.2 "Stage 1 Shareability attributes", these contain the
        //! shareability attributes of the descriptor OA for normal-cacheable
        //! memory.

        /// Non-shareable
        pub const NON_SHAREABLE: u64 = 0b00;
        /// Outer-shareable
        pub const OUTER_SHAREABLE: u64 = 0b10;
        /// Inner-shareable
        pub const INNER_SHAREABLE: u64 = 0b11;
    }

    /// Per "Figure D8-14 VMSAv8-64 Block descriptor formats" of ARM DDI0487L.b,
    /// subfigure "4KB, 16KB, and 64KB granules, 48-bit OA", the Output address
    /// is bits [47:n], and:
    ///
    /// > For the 4KB granule size, the level 1 descriptor n is 30,
    /// > and the level 2 descriptor n is 21.
    pub const BLOCK_BITS_1GB: u64 = 30;

    /// Per "Figure D8-14 VMSAv8-64 Block descriptor formats" of ARM DDI0487L.b,
    /// subfigure "4KB, 16KB, and 64KB granules, 48-bit OA", the Output address
    /// is bits [47:n], and:
    ///
    /// > For the 4KB granule size, the level 1 descriptor n is 30,
    /// > and the level 2 descriptor n is 21.
    pub const BLOCK_BITS_2MB: u64 = 21;

    // TODO:

    pub const BLOCK_BITS_512GB: u64 = 39;
    pub const PAGE_BITS_4KB: u64 = 12;

    /// Per "Table D8-52 Stage 1 VMSAv8-64 Block and Page descriptor fields" and
    /// "Figure D8-14 VMSAv8-64 Block descriptor formats" of ARM DDI0487L.b;
    /// specifically subfigure "4KB, 16KB, and 64KB granules, 48-bit OA"
    pub fn block_descriptor(level: usize, addr: u64, attr_index: u64) -> u64 {
        // Per Table D8-48, Condition for descriptor_type::BLOCK is level != 3.
        assert!(level != 3);

        let upper_attributes: u64 = 0;

        let shareability = if attr_index == s1_mair_attr_index::MT_NORMAL {
            // Match what the seL4 kernel uses for its page tables, which
            // is especially necessary for SMP booting which relies on it
            // for coherency. See the comment in seL4 `release_secondary_cpus()`.
            shareability_attributes::INNER_SHAREABLE
        } else {
            // Per $R_{PYFVQ}$:
            // > If a region is mapped as Device memory or Normal Non-cacheable
            // > memory after all enabled translation stages, then the region
            // > has an effective Shareability attribute of Outer Shareable.
            //
            // We override the value we place in here to OUTER_SHAREABLE to match
            // how the hardware behaves. This is not necessary but for clarity.
            shareability_attributes::OUTER_SHAREABLE
        };

        // AP[2:1], which we set as 0b00 for read/write access:
        //   stage 1: 0b00 is {PrivRead, PrivWrite} and we are EL1
        //   stage 2: 0b00 is RW for EL2 and no perms for EL1.
        const AP_KERNEL_RW: u64 = 0b00;

        // bit[11] is the not global (nG) field, we leave as 0 (global).
        // bit[10] is the access flag; depending on FEAT_HAFDBS, when software
        //         manages the AF memory accesses to the page/block when AF=0
        //         raise an Access Fault; when hardware manages the AF it will
        //         become 1.
        // bit[9:8] is SH[1:0] containing stage 1 shareability attributes
        // bit[7:6] contains AP[2:1]
        // bit[5] is RES0
        // bit[4:2] contains AttrIndex
        let lower_attributes: u64 =
            (1 << 10) | (AP_KERNEL_RW << 6) | (shareability << 8) | (attr_index << 2);

        // bits[47:n]
        let output_address: u64 = addr
            & !mask(match level {
                1 => BLOCK_BITS_1GB,
                2 => BLOCK_BITS_2MB,
                _ => panic!("unsupported level {level} for block descriptor"),
            });

        // address must not have bits above 47 set.
        assert!(addr & mask(48) == addr);

        // bits[63:50] describing the "Upper attributes" are left at 0.
        // bits[49:48] are RES0
        // bits[47:n] contain the Output address
        // bits[n-1:12] are RES0
        // bits[11:2] contain the "Lower attributes"
        // bits[1:0] contains the descriptor type
        upper_attributes | output_address | lower_attributes | descriptor_type::BLOCK
    }

    /// Per "Table D8-52 Stage 1 VMSAv8-64 Block and Page descriptor fields" and
    /// "Figure D8-15 VMSAv8-64 Page descriptor formats" of ARM DDI0487L.b;
    /// specifically subfigure "4KB granule 48-bit OA".
    pub fn page_descriptor(addr: u64, attr_index: u64) -> u64 {
        // The main difference between a page descriptor and block descriptor
        // is in the size of the output address (OA) and in the descriptor type.

        let upper_attributes: u64 = 0;

        let shareability = if attr_index == s1_mair_attr_index::MT_NORMAL {
            // Match what the seL4 kernel uses for its page tables, which
            // is especially necessary for SMP booting which relies on it
            // for coherency.
            shareability_attributes::INNER_SHAREABLE
        } else {
            // Per $R_{PYFVQ}$:
            // > If a region is mapped as Device memory or Normal Non-cacheable
            // > memory after all enabled translation stages, then the region
            // > has an effective Shareability attribute of Outer Shareable.
            // We override the value we place in here to OUTER_SHAREABLE to match
            // how the hardware behaves.
            shareability_attributes::OUTER_SHAREABLE
        };

        // AP[2:1], which we set as 0b00 for read/write access:
        //   stage 1: 0b00 is {PrivRead, PrivWrite} and we are EL1/El2 (priv)
        const AP_KERNEL_RW: u64 = 0b00;

        // bit[11] is the not global (nG) field, we leave as 0 (global).
        // bit[10] is the access flag; depending on FEAT_HAFDBS, when software
        //         manages the AF memory accesses to the page/block when AF=0
        //         raise an Access Fault; when hardware manages the AF it will
        //         become 1.
        // bit[9:8] is SH[1:0] containing stage 1 shareability attributes
        // bit[7:6] contains AP[2:1]
        // bit[5] is RES0
        // bit[4:2] contains AttrIndex
        let lower_attributes: u64 =
            (1 << 10) | (AP_KERNEL_RW << 6) | (shareability << 8) | (attr_index << 2);

        // bits[47:12]
        let output_address: u64 = addr & !mask(12);

        // address must not have bits above 47 set.
        assert!(addr & mask(48) == addr);

        // bits[63:50] describing the "Upper attributes" are left at 0.
        // bits[49:48] are RES0
        // bits[47:12] contain the Output address
        // bits[11:2] contain the "Lower attributes"
        // bits[1:0] contains the descriptor type
        upper_attributes | output_address | lower_attributes | descriptor_type::PAGE
    }

    /// Per "Table D8-50 Stage 1 VMSAv8-64 Table descriptor fields" and
    /// "Figure D8-12 VMSAv8-64 Table descriptor formats" of ARM DDI0487L.b;
    /// specifically subfigure "4KB, 16KB, and 64KB granules, 48-bit OA"
    pub fn table_descriptor(addr: *const u8) -> u64 {
        // Per Table D8-48, Condition for descriptor_type::TABLE is level != 3.

        let addr: u64 = addr.addr().try_into().expect("usize in u64");

        // We don't set any of these attributes, most are hardware-feature conditional
        let attributes: u64 = 0;

        // address must not have bits above 47 or below 12 set
        assert!(addr & mask(12) == 0x0);
        assert!(addr & mask(48) == addr);

        let next_level_table_address = addr;

        // bits[63:59] are "Attributes"
        // bits[58:51] are ignored
        // bits[50:48] are RES0
        // bits[47:m] is the next-level table address
        //  note: here m=12 for 4KB granule
        // bits[m-1:12] are RES0
        //  so this doesn't exist for 4KB granule
        // bits[11:2] are ignored
        // bits[1:0] contain the descriptor type
        attributes | next_level_table_address | descriptor_type::TABLE
    }
}

mod riscv64 {
    pub(crate) const BLOCK_BITS_1GB: u64 = 30;
    pub(crate) const BLOCK_BITS_2MB: u64 = 21;
    pub(crate) const PAGE_BITS_4K: u64 = 12;

    pub(crate) const PAGE_TABLE_INDEX_BITS: u64 = 9;
    pub(crate) const PAGE_SHIFT: u64 = 12;
    /// This sets the page table entry bits: D,A,X,W,R.
    pub(crate) const PTE_TYPE_BITS: u64 = 0b11001110;
    // TODO: where does this come from?
    pub(crate) const PTE_TYPE_TABLE: u64 = 0;
    pub(crate) const PTE_TYPE_VALID: u64 = 1;

    pub(crate) const PTE_PPN0_SHIFT: u64 = 10;

    /// Due to RISC-V having various virtual memory setups, we have this generic function to
    /// figure out the page-table index given the total number of page table levels for the
    /// platform and which level we are currently looking at.
    pub fn pt_index(pt_levels: usize, addr: u64, level: usize) -> usize {
        let pt_index_bits = PAGE_TABLE_INDEX_BITS * (pt_levels - level) as u64;
        let idx = (addr >> (pt_index_bits + PAGE_SHIFT)) % 512;

        idx as usize
    }

    /// Generate physical page number given an address
    pub fn pte_ppn(addr: u64) -> u64 {
        (addr >> PAGE_SHIFT) << PTE_PPN0_SHIFT
    }

    pub fn pte_next(addr: *const u8) -> u64 {
        pte_ppn(addr as u64) | PTE_TYPE_TABLE | PTE_TYPE_VALID
    }

    pub fn pte_leaf(addr: u64) -> u64 {
        pte_ppn(addr) | PTE_TYPE_BITS | PTE_TYPE_VALID
    }
}

/// RISC-V 64 page tables for our purposes uses the Sv39 translation scheme
/// (3-level page tables).
///
/// It is split into two halves: the Upper/Kernel part of the page tables,
/// which matches the format seL4 expects. The lower half contains an
/// identity mapped region for the loader.
///
/// ```txt
///            (512 GiB)
///   512 +---- Level 1 ---+ 2^39
///       |                |
///       |     (empty)    |
///       |                |
///   k+1 +----------------+                   (1 GiB)
///       | Level 2 Kernel | ----------> +---- Level 2 ---+             +-------------+
///     k +----------------+             |                | ----------> | 2 MiB block |
///       |                |         511 |----------------|             +-------------+
///       |                |             |                | ----------> | 2 MiB block |
///       |                |         510 |----------------|             +-------------+
///       |                |             |                | ----------> | 2 MiB block |
///       |                |             |----------------|             +-------------
///       |                |                   (...)           (...)         (...)          Kernel Regions
///       |                |             |----------------|             +-------------+
///       |                |             |                | ----------> | 2 MiB block |
///       |                |         l+1 |----------------|             +-------------+
///       |                |             | Level 3 Kernel | ----+
///       |                |           l |----------------|     |
///       |                |             |                |     |           (2 MiB)
///       |                |             |                |     +-----> +-- Level 3 --+             +------------+
///       |                |             |                |             |             | ----------> | 4 KiB page |
///       |                |             |                |         511 |-------------|             +------------+
///       |                |             |     (empty)    |             |             | ----------> | 4 KiB page |
///       |     (empty)    |             |                |             |-------------|             +------------+
///       |                |             |                |             |             | ----------> | 4 KiB page |
///       |                |             |                |           m |-------------|             +------------+ p
///       |                |             |                |             |   (empty)   |
///       |                |             |                |             +-------------+
///       |                |             |                |
///       |                |           0 +----------------+
///       |                |
///       |                |
///       |                |
///       |                |
///       |                |
///     1 +----------------+
///       | Level 2 Loader | ---------->  +-- RAM
///     0 +----------------+
///
///
/// Where:
///      k = align_down(kernel_first_vaddr, 1GiB),
///      l = align_down(kernel_first_vaddr, 2MiB),
///      m = align_down(kernel_first_vaddr, 4KiB),
///      p = align_down(kernel_first_paddr, 4KiB),
/// ```
///
/// # Safety
/// - regions_ptr must be valid for as long as this function runs,
///   and regions_len must represent its length
/// - page_table_bytes must be aligned to PAGE_TABLE_SIZE
///
///
#[unsafe(no_mangle)]
pub unsafe extern "C" fn riscv64_setup_pagetables(
    kernel_first_vaddr: u64,
    kernel_first_paddr: u64,
    // In-out param; storage and input
    regions_ptr: *mut Region,
    regions_len: usize,
    // Storage used for page tables
    page_table_bytes: &mut [[MaybeUninit<u8>; PAGE_TABLE_SIZE]; MAX_NUM_PAGE_TABLES],
) -> *const u8 {
    use riscv64::{pt_index, pte_leaf, pte_next, BLOCK_BITS_1GB, BLOCK_BITS_2MB, PAGE_BITS_4K};

    const PAGE_TABLE_ENTRIES: usize = PAGE_TABLE_SIZE / mem::size_of::<u64>();

    let (pt_temporaries, mut serialise_page_table_to_paddr) =
        make_helper_pt_serialisers::<PAGE_TABLE_ENTRIES>(page_table_bytes);

    struct Config {
        riscv_pt_levels: usize,
    }
    let config = Config { riscv_pt_levels: 3 };

    let num_pt_levels = config.riscv_pt_levels;
    assert!(num_pt_levels == 3);

    // Manufacture the constants as per the diagram.
    let k = align_down(kernel_first_vaddr, BLOCK_BITS_1GB);
    let l = align_down(kernel_first_vaddr, BLOCK_BITS_2MB);
    let m = align_down(kernel_first_vaddr, PAGE_BITS_4K);
    let p = align_down(kernel_first_paddr, PAGE_BITS_4K);

    // Manufacture the kernel page tables
    let kernel_lvl2_pt_paddr = {
        let mut paddr = p;
        let index_l = pt_index(num_pt_levels, l, 2);

        let lvl2_pte_entry = if kernel_first_vaddr.is_multiple_of(1 << BLOCK_BITS_2MB) {
            assert!(paddr.is_multiple_of(1 << BLOCK_BITS_2MB));
            let pte = pte_leaf(paddr);
            paddr += 1 << BLOCK_BITS_2MB;
            pte
        } else {
            let lvl3_pt_kernel = &mut pt_temporaries[0];

            let index_m = pt_index(num_pt_levels, m, 3);

            for index in index_m..512 {
                lvl3_pt_kernel[index] = pte_leaf(paddr);
                paddr += 1 << PAGE_BITS_4K;
            }

            let kernel_lvl3_pt_paddr = serialise_page_table_to_paddr(lvl3_pt_kernel);
            pte_next(kernel_lvl3_pt_paddr)
        };

        let lvl2_pt_kernel = &mut pt_temporaries[0];
        lvl2_pt_kernel[index_l] = lvl2_pte_entry;

        for index in (index_l + 1)..512 {
            lvl2_pt_kernel[index] = pte_leaf(paddr);
            paddr += 1 << BLOCK_BITS_2MB;
        }

        serialise_page_table_to_paddr(lvl2_pt_kernel)
    };

    let boot_lvl1_pt = {
        let identity_mapped_regions: &mut [Region] = {
            let regions = unsafe { slice::from_raw_parts_mut(regions_ptr, regions_len) };

            for region in regions.iter_mut() {
                // RISC-V ignores attributes
                region.arch_attrs.raw = 0;
            }

            // Need to use 'sort_unstable_by_key' as sort_by_key is not in-place.
            regions.sort_unstable_by_key(|region| region.start);

            regions
        };

        setup_identity_page_tables::<3, _, Riscv64PtLayout, _>(
            identity_mapped_regions,
            pt_temporaries,
            &mut serialise_page_table_to_paddr,
        )
    };

    let index_k = pt_index(num_pt_levels, k, 1);
    assert_eq!(boot_lvl1_pt[index_k], 0);
    boot_lvl1_pt[index_k] = pte_next(kernel_lvl2_pt_paddr);

    serialise_page_table_to_paddr(boot_lvl1_pt)
}

const MAX_NUM_PAGE_TABLES: usize = 64;
const NUM_TEMPORARIES: usize = 4;

pub trait ArchPtLayout<const LEVELS: usize> {
    const LEVELS: usize = LEVELS;

    const MIN_LEVEL: usize;
    const LEVEL_BITS: [(u32, u32); LEVELS];

    fn leaf_entry(level: usize, address: usize, attributes: u64) -> u64;
    fn table_entry(level: usize, address: *const u8) -> u64;
}

struct AArch64PtLayout;

impl ArchPtLayout<4> for AArch64PtLayout {
    const MIN_LEVEL: usize = 1;
    const LEVEL_BITS: [(u32, u32); 4] = [(39, 9), (30, 9), (21, 9), (12, 9)];

    fn leaf_entry(level: usize, address: usize, attributes: u64) -> u64 {
        assert!(level < Self::LEVELS);

        let address = address.try_into().unwrap();

        if level == 3 {
            aarch64::page_descriptor(address, attributes)
        } else {
            aarch64::block_descriptor(level, address, attributes)
        }
    }

    fn table_entry(_level: usize, address: *const u8) -> u64 {
        aarch64::table_descriptor(address)
    }
}

struct Riscv64PtLayout;

impl ArchPtLayout<3> for Riscv64PtLayout {
    const MIN_LEVEL: usize = 0;
    const LEVEL_BITS: [(u32, u32); 3] = [(30, 9), (21, 9), (12, 9)];

    fn leaf_entry(level: usize, address: usize, attributes: u64) -> u64 {
        assert!(level < Self::LEVELS);
        assert_eq!(attributes, 0);

        let address = address.try_into().unwrap();

        riscv64::pte_leaf(address)
    }

    fn table_entry(_level: usize, address: *const u8) -> u64 {
        riscv64::pte_next(address)
    }
}

fn setup_identity_page_tables<
    'a,
    const LEVELS: usize,
    const PAGE_TABLE_ENTRIES: usize,
    LAYOUT,
    SerialiseFn,
>(
    identity_mapped_regions: &mut [Region],
    pt_temporaries: &'a mut [[u64; PAGE_TABLE_ENTRIES]; NUM_TEMPORARIES],
    mut serialise_page_table_to_paddr: SerialiseFn,
) -> &'a mut [u64; PAGE_TABLE_ENTRIES]
where
    SerialiseFn: FnMut(&mut [u64; PAGE_TABLE_ENTRIES]) -> *const u8,
    LAYOUT: ArchPtLayout<LEVELS>,
{
    // Manufacture the RAM page tables, which is a little bit more complicated.

    // We maintain three active page tables, which contain our previous
    // known page table data. As we process regions in ascending order,
    // once we have exceeded the bounds of the current reservation we
    // can simply push to the page_table_bytes storage and insert into
    // the parent PT the descriptor.

    // We never actually use level 0 here, but it is nice to have because
    // then the indices are the same as the level.
    let pts_by_level = pt_temporaries.get_disjoint_mut([0, 1, 2, 3]).unwrap();

    let mut iter =
        AlignedRegionsIter::new(identity_mapped_regions.iter(), LAYOUT::LEVEL_BITS).peekable();

    while let Some((level, level_indices, current_addr, attributes)) = iter.next() {
        assert!(level >= LAYOUT::MIN_LEVEL);

        assert!(pts_by_level[level][level_indices[level]] == 0);

        pts_by_level[level][level_indices[level]] =
            LAYOUT::leaf_entry(level, current_addr, attributes);

        // Invariant: the page tables in pts_by_level are either:
        // (1) for the current level_indices, or
        // (2) are empty/invalid and for a lower level.
        // Similar, the level indices in our array are only meaningful
        // from [0..=level].
        //
        // Hence, when moving around, we only need to care about page tables
        // in the range [0, level) inclusive, and can ignore those on
        // lower levels.
        // We start from the lowest level (parent) checking if the indices
        // prefix (i.e. it, or any above it) have changed. Note that
        // checking just the index would be invalid, in the case of say a
        // [0, 0, 1, 0] -> [0, 0, 2, 0] change where level=3, as the
        // level=2 row has changed, so our level=3 page table must be
        // written out.
        // We start from the parent and not the current level, because
        // the change from [0, 0, 1, 0] -> [0, 0, 1, 1] should not write
        // out the page table. (similarly, [0, 0, 1, X] -> [0, 0, 1, X]
        // for level=2).
        // We don't need to care if next_level is higher than the current
        // level, as this still means the current page table is valid.

        for level in (LAYOUT::MIN_LEVEL..level).rev() {
            // Two cases where we need to write out the page tables:
            // either we are reaching the end (iter.peek() = None)
            // or if the next one has different page tables to us.
            let changed = match iter.peek() {
                None => true,
                Some((_, next_level_indices, _, _)) => {
                    level_indices[0..=level] != next_level_indices[0..=level]
                }
            };

            // Flush the 'level + 1' (the entry in the current level's PT)
            // into the 'level' PT (next level up)
            // We could have written instead this for loop as
            // `for level in (MIN_LEVEL+1..=level)`
            // and then used `let parent_level = level - 1`.
            if changed {
                let pt_paddr = serialise_page_table_to_paddr(pts_by_level[level + 1]);
                pts_by_level[level][level_indices[level]] = LAYOUT::table_entry(level, pt_paddr);
            }
        }
    }

    &mut pt_temporaries[LAYOUT::MIN_LEVEL]
}

fn make_helper_pt_serialisers<const PAGE_TABLE_ENTRIES: usize>(
    page_table_bytes: &mut [[MaybeUninit<u8>; PAGE_TABLE_SIZE]; MAX_NUM_PAGE_TABLES],
) -> (
    &mut [[u64; PAGE_TABLE_ENTRIES]; NUM_TEMPORARIES],
    impl FnMut(&mut [u64; PAGE_TABLE_ENTRIES]) -> *const u8,
) {
    // FIXME: Replace once https://github.com/rust-lang/rust/issues/90091 is merged
    let (page_table_bytes, pt_temporaries) = page_table_bytes
        .split_first_chunk_mut::<{ MAX_NUM_PAGE_TABLES - NUM_TEMPORARIES }>()
        .unwrap();

    let pt_temporaries = {
        let pt_temporaries: &mut [[MaybeUninit<u8>; PAGE_TABLE_SIZE]; NUM_TEMPORARIES] =
            pt_temporaries.try_into().unwrap();

        for pt in pt_temporaries.iter_mut() {
            for elem in pt {
                elem.write(0);
            }
        }

        // SAFETY: we just initialised it.
        let pt_temporaries = unsafe {
            mem::transmute::<
                &mut [[MaybeUninit<u8>; PAGE_TABLE_SIZE]; NUM_TEMPORARIES],
                &mut [[u8; PAGE_TABLE_SIZE]; NUM_TEMPORARIES],
            >(pt_temporaries)
        };

        // SAFETY:
        // - all bitpatterns of u8 can be represented in u8.
        // - alignment requirements are met by input requirements
        unsafe {
            assert!((pt_temporaries.as_ptr() as usize).is_multiple_of(PAGE_TABLE_SIZE));
            mem::transmute::<
                &mut [[u8; PAGE_TABLE_SIZE]; NUM_TEMPORARIES],
                &mut [[u64; PAGE_TABLE_ENTRIES]; NUM_TEMPORARIES],
            >(pt_temporaries)
        }
    };

    let serialise_page_table_to_paddr = {
        let page_tables_paddr_start: *const u8 = page_table_bytes.as_ptr().cast();

        assert!((page_tables_paddr_start as usize).is_multiple_of(PAGE_TABLE_SIZE));

        // This maintains the current end of the PT array.
        let mut next_pt_paddr = page_tables_paddr_start;
        let mut i = 0;

        move |page_table: &mut [u64; PAGE_TABLE_ENTRIES]| -> *const _ {
            let pt_paddr = next_pt_paddr;
            for (j, byte) in page_table
                .iter()
                .flat_map(|pte| pte.to_le_bytes())
                .enumerate()
            {
                page_table_bytes[i][j].write(byte);
            }

            next_pt_paddr = next_pt_paddr.wrapping_add(PAGE_TABLE_SIZE);
            i += 1;
            page_table.fill(0);

            if cfg!(test) {
                // HACK! For tests, we want stable page tables, but due to ASLR
                // we get random things every time. Instead, let's make the
                // paddr we return a relative-to-start-of-page-tables value.
                return unsafe { pt_paddr.offset_from(page_tables_paddr_start) } as *const _;
            }

            pt_paddr
        }
    };

    (pt_temporaries, serialise_page_table_to_paddr)
}

/// AArch64 loader page tables have two variations:
///  - Loader in EL2, then Stage 1 translations in use, so we have the
///    singular TTBR0_EL2 register containing the Level 0 table;
///    this allows virtual address in the range [0,2^48).
///  - Loader in EL1, then Stage 1 translations are in use, so we have both
///    the TTBR0_EL1 (covering vaddr in range [0,2^48)) and TTBR1_EL2 (
///    (covering vaddr in the range [2^64-2^48,2^64)), and containing
///    the "Level 0 Lower" page table, and "Level 0 Upper" page table
///    physical addresses respectively.
///
/// Thus, for EL2 loader, the singular Level 0 page table contains the table
/// descriptors for the "Level 1 Upper" and "Level 1 Lower" page tables.
/// For the EL1 loader, we instead have two Level 0 page tables, and
/// "Level 0 Lower" contains the "Level 1 Lower" descriptor, and "Level 0
/// Upper" contains the "Level 1 Upper" descriptor.
/// Otherwise, the page tables layout from Level 1 downwards are identical
/// (but not necessarily the layout within the page/table/block descriptors).
///
/// ```txt
///          (256 TiB)
///   512 +-- Level 0 --+ 2^48
///       |             |
///       |   (empty)   |
///       |             |
///   k+1 +-------------+                 (512 GiB)
///       | Level 1 Upr | ---------->  +-- Level 1 --+
///     k +-------------+              |             |
///       |             |              |   (empty)   |
///       |             |              |             |
///       |             |         l+1  +-------------+                 (1 GiB)
///       |             |              | Level 2 Upr | ----------> +-- Level 2 --+             +-------------+
///       |             |           l  +-------------+             |             | ----------> | 2 MiB block |
///       |             |              |             |         511 |-------------|             +-------------+
///       |             |              |   (empty)   |             |             | ----------> | 2 MiB block |
///       |             |              |             |         510 |-------------|             +-------------+
///       |             |              +-------------+             |             | ----------> | 2 MiB block |
///       |             |                                          |-------------|             +-------------+
///       |   (empty)   |                          Kernel Regions       (...)         (...)         (...)
///       |             |                                          |-------------|             +-------------+
///       |             |                                          |             | ----------> | 2 MiB block |
///       |             |                                        m |-------------|             +-------------+ p
///       |             |                                          |             |
///       |             |                                          |   (empty)   |
///       |             |                                          |             |
///       |             |                                        0 +-------------+
///       |             |
///       |             |
///       |             |
///     1 +-------------+                 (512 GiB)
///       | Level 1 Lwr | ---------->  +-- Level 1 --+
///     0 +-------------+              TODO: RAM.
///
///
/// Where:
///      k = align_down(kernel_first_vaddr, 512GiB),
///      l = align_down(kernel_first_vaddr, 1GiB),
///      m = align_down(kernel_first_vaddr, 2MiB),
///      p = align_down(kernel_first_paddr, 2MiB),
///      u = align_down(uart_base, 1GiB),
/// ```
///
/// # Safety
/// - regions_ptr must be valid for as long as this function runs,
///   and regions_len must represent its length
/// - page_table_bytes must be aligned to PAGE_TABLE_SIZE
///
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aarch64_setup_pagetables(
    kernel_first_vaddr: u64,
    kernel_first_paddr: u64,
    // In-out param; storage and input
    regions_ptr: *mut Region,
    regions_len: usize,
    // Storage used for page tables
    page_table_bytes: &mut [[MaybeUninit<u8>; PAGE_TABLE_SIZE]; MAX_NUM_PAGE_TABLES],
) -> AArch64ReturnValue {
    use aarch64::{
        block_descriptor, lvl0_index, lvl1_index, lvl2_index,
        s1_mair_attr_index::{MT_DEVICE_nGnRnE, MT_NORMAL},
        table_descriptor, BLOCK_BITS_1GB, BLOCK_BITS_2MB, BLOCK_BITS_512GB,
    };

    const PAGE_TABLE_ENTRIES: usize = PAGE_TABLE_SIZE / mem::size_of::<u64>();

    let (pt_temporaries, mut serialise_page_table_to_paddr) =
        make_helper_pt_serialisers::<PAGE_TABLE_ENTRIES>(page_table_bytes);

    // Manufacture the constants as per the diagram.
    let k = align_down(kernel_first_vaddr, BLOCK_BITS_512GB);
    let l = align_down(kernel_first_vaddr, BLOCK_BITS_1GB);
    let m = align_down(kernel_first_vaddr, BLOCK_BITS_2MB);
    let p = align_down(kernel_first_paddr, BLOCK_BITS_2MB);

    // Manufacture the kernel page tables, which is relatively straightforward.
    let kernel_lvl1_pt_paddr = {
        // First, the Level 2 Upr table.
        let lvl2_pt_paddr = {
            let lvl2_pt_kernel = &mut pt_temporaries[0];

            let mut vaddr = m;
            let mut paddr = p;
            while lvl1_index(m) == lvl1_index(vaddr) {
                lvl2_pt_kernel[lvl2_index(vaddr)] = block_descriptor(2, paddr, MT_NORMAL);

                vaddr += 1 << BLOCK_BITS_2MB;
                paddr += 1 << BLOCK_BITS_2MB;
            }

            serialise_page_table_to_paddr(lvl2_pt_kernel)
        };

        // Then, the Level 1 Upr table.
        let lvl1_pt_kernel = &mut pt_temporaries[0];
        lvl1_pt_kernel[lvl1_index(l)] = table_descriptor(lvl2_pt_paddr);

        serialise_page_table_to_paddr(lvl1_pt_kernel)
    };

    let ram_lvl1_pt_paddr = {
        let identity_mapped_regions: &mut [Region] = {
            let regions = unsafe { slice::from_raw_parts_mut(regions_ptr, regions_len) };

            for region in regions.iter_mut() {
                // SAFETY: We expect users to set is_ram appropriately.
                region.arch_attrs.raw = if unsafe { region.arch_attrs.is_ram } {
                    // FIXME: For now, RAM is also mapped as DEVICE memory.
                    MT_NORMAL
                } else {
                    MT_DEVICE_nGnRnE
                };
            }

            // Need to use 'sort_unstable_by_key' as sort_by_key is not in-place.
            regions.sort_unstable_by_key(|region| region.start);

            regions
        };

        let top_pt = setup_identity_page_tables::<4, _, AArch64PtLayout, _>(
            identity_mapped_regions,
            pt_temporaries,
            &mut serialise_page_table_to_paddr,
        );

        serialise_page_table_to_paddr(top_pt)
    };

    struct Config {
        hypervisor: bool,
    }
    let config = Config { hypervisor: true };

    // Depending on whether we are in hypervisor mode, we either need to
    // return the TTBR0_EL2 or TTBR[0,1]_EL1 values. We return u64::MAX
    // so as to return garbage - an unaligned address outside of physical
    // memory.
    if config.hypervisor {
        // Manufacture the Level 0 table, containing the kernel table
        // and the RAM tables.

        let ttbr0_el2_pt = &mut pt_temporaries[0];

        assert!(lvl0_index(k) != lvl0_index(0));
        ttbr0_el2_pt[lvl0_index(k)] = table_descriptor(kernel_lvl1_pt_paddr);
        ttbr0_el2_pt[lvl0_index(0)] = table_descriptor(ram_lvl1_pt_paddr);

        let ttbr0_el2 = serialise_page_table_to_paddr(ttbr0_el2_pt);

        AArch64ReturnValue {
            ttbr0_el2,
            ttbr0_el1: AArch64ReturnValue::INVALID,
            ttbr1_el1: AArch64ReturnValue::INVALID,
        }
    } else {
        let [ttbr0_el1_pt, ttbr1_el1_pt] = pt_temporaries.get_disjoint_mut([0, 1]).unwrap();

        // Kernel in TTBR1 (Upper)
        ttbr1_el1_pt[lvl0_index(k)] = table_descriptor(kernel_lvl1_pt_paddr);
        // Identity-mapped RAM in TTBR0 (Lower)
        ttbr0_el1_pt[lvl0_index(0)] = table_descriptor(ram_lvl1_pt_paddr);

        let ttbr0_el1 = serialise_page_table_to_paddr(ttbr0_el1_pt);
        let ttbr1_el1 = serialise_page_table_to_paddr(ttbr1_el1_pt);

        AArch64ReturnValue {
            ttbr0_el2: AArch64ReturnValue::INVALID,
            ttbr0_el1,
            ttbr1_el1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern crate std;
    use std::unreachable;
    use std::vec;
    use std::vec::Vec;

    use shared_types::MmuRegionArchAttrs as RegionArchAttrs;

    // Exclusive [start, end)
    #[derive(Clone, Debug, PartialEq)]
    struct WalkRegion {
        v_start: u64,
        v_end: u64,
        p_start: u64,
        p_end: u64,
        // This includes *all* page table attributes
        arch_value: u64,
    }

    fn merge_regions(regions: &mut Vec<WalkRegion>) {
        if regions.len() == 0 {
            return;
        }

        let mut top = 0;
        for i in 1..regions.len() {
            if regions[top].p_end == regions[i].p_start
                && regions[top].v_end == regions[i].v_start
                && regions[top].arch_value == regions[i].arch_value {

                regions[top].p_end = regions[i].p_end;
                regions[top].v_end = regions[i].v_end;
            } else {
                top += 1;
                regions[top] = regions[i].clone();
            }
        }

        regions.truncate(top + 1);
    }

    fn aarch64_walk_pt_level_order(
        level: usize,
        pte: u64,
        vaddr: &mut u64,
        pts: &[[u64; 512]],
        regions: &mut Vec<WalkRegion>,
    ) {
        use aarch64::descriptor_type;

        // Level is [0, 4)
        assert!(level < 4);

        let v_start = *vaddr;
        let size = 1
            << match level {
                0 => aarch64::BLOCK_BITS_512GB,
                1 => aarch64::BLOCK_BITS_1GB,
                2 => aarch64::BLOCK_BITS_2MB,
                3 => aarch64::PAGE_BITS_4KB,
                _ => unreachable!(),
            };
        *vaddr += size;
        let v_end = *vaddr;

        if pte == 0 {
            return;
        }

        let pte_type = pte & 0b11;
        // bits [47: 12]
        let pte_oa = pte & 0xfffffffff000;
        let pte_attrs = pte & !0xfffffffff000;

        if level == 3 {
            assert!(pte_type == descriptor_type::PAGE);
        }

        if level != 3 && pte_type == descriptor_type::TABLE {
            let next_level_pt_idx = (pte_oa as usize) / PAGE_TABLE_SIZE;

            let mut vaddr = v_start;
            for &child_pte in pts[next_level_pt_idx].iter() {
                aarch64_walk_pt_level_order(level + 1, child_pte, &mut vaddr, pts, regions);
            }
        } else {
            regions.push(WalkRegion {
                v_start,
                v_end,
                p_start: pte_oa,
                p_end: pte_oa + size,
                arch_value: pte_attrs,
            });
        }
    }

    fn aarch64_walk_pt_gather_regions(pts: &[[u64; 512]], root_idx: usize) -> Vec<WalkRegion> {
        let mut regions = vec![];
        let mut vaddr = 0;
        for &pte in pts[root_idx].iter() {
            aarch64_walk_pt_level_order(0, pte, &mut vaddr, pts, &mut regions);
        }

        merge_regions(&mut regions);
        regions
    }

    fn riscv64_walk_pt_level_order(
        level: usize,
        pte: u64,
        vaddr: &mut u64,
        pts: &[[u64; 512]],
        regions: &mut Vec<WalkRegion>,
    ) {

        // Level is [0, 3)
        assert!(level < 3);

        let v_start = *vaddr;
        let size = 1
            << match level {
                0 => riscv64::BLOCK_BITS_1GB,
                1 => riscv64::BLOCK_BITS_2MB,
                2 => riscv64::PAGE_BITS_4K,
                _ => unreachable!(),
            };
        *vaddr += size;
        let v_end = *vaddr;

        if pte == 0 {
            return;
        }

        assert_eq!(pte & riscv64::PTE_TYPE_VALID, riscv64::PTE_TYPE_VALID);

        // if all RWX are zero, then table
        let pte_is_leaf = pte & 0b1110 != 0;
        // bits [53:10], shifted left so it is the actual address
        let pte_oa = (pte & 0x1ffffffffffc00) << (12 - 10);
        let pte_attrs = pte & !0x1ffffffffffc00;

        if level == 2 {
            assert!(pte_is_leaf);
        }

        if level != 2 && !pte_is_leaf {
            let next_level_pt_idx = (pte_oa as usize) / PAGE_TABLE_SIZE;

            let mut vaddr = v_start;
            for &child_pte in pts[next_level_pt_idx].iter() {
                riscv64_walk_pt_level_order(level + 1, child_pte, &mut vaddr, pts, regions);
            }
        } else {
            regions.push(WalkRegion {
                v_start,
                v_end,
                p_start: pte_oa,
                p_end: pte_oa + size,
                arch_value: pte_attrs,
            });
        }
    }

    fn riscv64_walk_pt_gather_regions(pts: &[[u64; 512]], root_idx: usize) -> Vec<WalkRegion> {
        let mut regions = vec![];
        let mut vaddr = 0;
        for &pte in pts[root_idx].iter() {
            riscv64_walk_pt_level_order(0, pte, &mut vaddr, pts, &mut regions);
        }

        merge_regions(&mut regions);
        regions
    }

    #[test]
    fn qemu_aarch64() {
        #[repr(align(4096))]
        struct PtBytes([[MaybeUninit<u8>; 4096]; MAX_NUM_PAGE_TABLES]);

        let mut regions = [
            Region {
                start: 0x60000000,
                top: 0xc0000000 - 1,
                arch_attrs: RegionArchAttrs { is_ram: true },
            },
            // UART
            Region {
                start: 0x9000000,
                top: 0x9000fff,
                arch_attrs: RegionArchAttrs { is_ram: false },
            },
        ];

        //     // FIXME: Derive from the kernel build system.
        //     if let Some(uart_base) = read_symbol_maybe(elf, "uart_addr") {
        //         let uart_base = align_down(uart_base, PAGE_BITS_4KB);
        //         regions.push((
        //             PlatformConfigRegion {
        //                 start: uart_base,
        //                 end: uart_base + (1 << PAGE_BITS_4KB),
        //             },
        //             MT_DEVICE_nGnRnE,
        //         ));
        //     }
        //     // FIXME: This is currently assuming implementation details of the BCM2711/
        //     //        Raspberry Pi 4B spin table implementation, as it is the only
        //     //        platform we have that uses spin tables. Specifically, that
        //     //        it is always located at the 0 page.
        //     if elf.find_symbol("cpus_release_addr").is_ok() {
        //         regions.push((
        //             PlatformConfigRegion {
        //                 start: 0x0,
        //                 end: 1 << PAGE_BITS_4KB,
        //             },
        //             MT_DEVICE_nGnRnE,
        //         ));
        //     }

        let mut page_table_bytes = PtBytes([[MaybeUninit::zeroed(); _]; _]);

        let pt_bases = unsafe {
            aarch64_setup_pagetables(
                /* kernel_first_vaddr */ 0x8060000000,
                /* kernel_first_paddr */ 0x60000000,
                regions.as_mut_ptr(),
                regions.len(),
                &mut page_table_bytes.0,
            )
        };

        let page_tables = unsafe {
            mem::transmute::<
                [[MaybeUninit<u8>; PAGE_TABLE_SIZE]; MAX_NUM_PAGE_TABLES],
                [[u64; 512]; MAX_NUM_PAGE_TABLES],
            >(page_table_bytes.0)
        };

        assert_eq!(pt_bases.ttbr0_el1, AArch64ReturnValue::INVALID);
        assert_eq!(pt_bases.ttbr1_el1, AArch64ReturnValue::INVALID);
        assert_ne!(pt_bases.ttbr0_el2, AArch64ReturnValue::INVALID);

        let root_addr = pt_bases.ttbr0_el2 as usize;

        let walk_regions =
            aarch64_walk_pt_gather_regions(&page_tables, root_addr / PAGE_TABLE_SIZE);

        assert_eq!(
            walk_regions,
            vec![
                // UART
                WalkRegion {
                    v_start: 0x9000000,
                    v_end: 0x9001000,
                    p_start: 0x9000000,
                    p_end: 0x9001000,
                    arch_value: 0x603,
                },
                // RAM
                WalkRegion {
                    v_start: 0x60000000,
                    v_end: 0xc0000000,
                    p_start: 0x60000000,
                    p_end: 0xc0000000,
                    arch_value: 0x711,
                },
                // seL4
                WalkRegion {
                    v_start: 0x8060000000,
                    v_end: 0x8080000000,
                    p_start: 0x60000000,
                    p_end: 0x80000000,
                    arch_value: 0x711,
                },
            ]
        );
    }

    #[test]
    fn qemu_riscv64() {
        #[repr(align(4096))]
        struct PtBytes([[MaybeUninit<u8>; 4096]; MAX_NUM_PAGE_TABLES]);

        let mut regions = [
            // RAM
            Region {
                start: 0x80200000,
                top: 0x100000000 - 1,
                arch_attrs: RegionArchAttrs { is_ram: true },
            },
        ];

        let mut page_table_bytes = PtBytes([[MaybeUninit::zeroed(); _]; _]);

        let boot_lvl1_pt = unsafe {
            riscv64_setup_pagetables(
                /* kernel_first_vaddr */ 0xffffffff80200000,
                /* kernel_first_paddr */ 0x80200000,
                regions.as_mut_ptr(),
                regions.len(),
                &mut page_table_bytes.0,
            )
        };

        let page_tables = unsafe {
            mem::transmute::<
                [[MaybeUninit<u8>; PAGE_TABLE_SIZE]; MAX_NUM_PAGE_TABLES],
                [[u64; 512]; MAX_NUM_PAGE_TABLES],
            >(page_table_bytes.0)
        };

        let root_addr = boot_lvl1_pt as usize;

        let walk_regions =
            riscv64_walk_pt_gather_regions(&page_tables, root_addr / PAGE_TABLE_SIZE);

        // FIXME: why is RAM split in two?
        assert_eq!(
            walk_regions,
            vec![
                // RAM
                WalkRegion {
                    v_start: 0x80200000,
                    v_end: 0x100000000,
                    p_start: 0x80200000,
                    p_end: 0x100000000,
                    arch_value: 0xcf,
                },
                // seL4
                WalkRegion {
                    // This is the same as 0xffffffff80200000 but with leading chopped
                    v_start: 0x7f80200000,
                    v_end: 0x7fc0000000,
                    p_start: 0x80200000,
                    p_end: 0xc0000000,
                    arch_value: 0xcf,
                },
            ]
        );
    }
}
