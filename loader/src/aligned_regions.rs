//
// Copyright 2026, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

//! This function provides an iterator which is useful for generating
//! page tables in one pass, with a constant amount of "extra" space for
//! bookkeeping. It transforms a set of discontiguous regions that cover
//! multiple levels of page table structures into a set of aligned regions,
//! useful for filling out paging structures. An additional piece of information
//! we need to maintain is when we move between levels.

use core::array;
use core::cmp::min;

use crate::Region;

/// Bits from [start, end)
/// Note: Bit indices are 'u32' as this is what rust tends to use for usize::BITS,
/// and for checked_shl, and otherwise. This makes our life simpler.
fn bits_of_range(value: usize, start: u32, end: u32) -> usize {
    assert!(start < end);
    assert!(start < usize::BITS);
    assert!(end <= usize::BITS);

    // Handle the maximum-shift case.
    let mask = if let Some(bit) = 1usize.checked_shl(end) {
        bit - 1
    } else {
        debug_assert!(end == usize::BITS);
        usize::MAX
    };

    (value & mask) >> start
}

fn indices_of_level<const LEVELS: usize>(
    // [size_bits, count_bits)
    level_bits: &[(u32, u32); LEVELS],
    level: usize,
    value: usize,
) -> usize {
    let (size_bits, count_bits) = level_bits[level];

    bits_of_range(value, size_bits, size_bits + count_bits)
}

#[must_use = "iterators are lazy and do nothing unless consumed"]
pub struct AlignedRegionsIter<'a, I, const LEVELS: usize>
where
    I: Iterator<Item = &'a Region>,
{
    /// Array of (size_bits, count_bits), descending, for each level
    level_bits: [(u32, u32); LEVELS],
    input_regions_iter: I,
    current_input_region: Option<&'a Region>,
    current_addr: usize,
}

impl<'a, I, const LEVELS: usize> AlignedRegionsIter<'a, I, LEVELS>
where
    I: Iterator<Item = &'a Region>,
{
    // TODO: method on iter?
    pub fn new(iter: I, level_bits: [(u32, u32); LEVELS]) -> Self {
        for ((upper_size, _), (lower_size, lower_count)) in
            level_bits.windows(2).map(|s| (s[0], s[1]))
        {
            assert!(upper_size == lower_size + lower_count);
        }

        Self {
            level_bits,
            input_regions_iter: iter,
            current_input_region: None,
            current_addr: 0,
        }
    }
}

impl<'a, I, const LEVELS: usize> Iterator for AlignedRegionsIter<'a, I, LEVELS>
where
    I: Iterator<Item = &'a Region>,
{
    type Item = (usize, [usize; LEVELS], usize, u64);

    fn next(&mut self) -> Option<Self::Item> {
        let region = match self.current_input_region {
            Some(r) => r,
            None => {
                let Some(region) = self.input_regions_iter.next() else {
                    // We exit our iterator here as we have no more work to do.
                    return None;
                };

                assert!(region.start < region.top);

                self.current_input_region = Some(region);
                // Guarantees the loop invariant.
                self.current_addr = region.start;

                region
            }
        };

        let current_addr = self.current_addr;

        // Loop invariant.
        assert!(current_addr < region.top);

        let size = region.top.checked_sub(current_addr).unwrap() + 1;
        let size_bits = size.ilog2();
        // FIXME: Once MSRV is > 1.97, use .lowest_one() method.
        let align_bits = if current_addr == 0 {
            size_bits
        } else {
            current_addr.trailing_zeros()
        };

        // The correct pt size bits we can use it the smallest of the size
        // and the alignment; we can't use a 21-bit aligned region if
        // we have a 15-bit region, since it would overrun.
        let align_bits = min(align_bits, size_bits);

        let level = self
            .level_bits
            .map(|(size, _)| size)
            .iter()
            .position(|&level_size_bits| align_bits >= level_size_bits)
            .expect("bad input; regions should be aligned to at least the lowest level");

        let level_indices: [usize; LEVELS] =
            array::from_fn(|level| indices_of_level(&self.level_bits, level, current_addr));

        let next_addr = current_addr.wrapping_add(1 << self.level_bits[level].0);

        if next_addr > region.top || next_addr == 0 {
            self.current_input_region = None;
        }

        self.current_addr = next_addr;

        // SAFETY: raw contains all valid bitpatterns
        let raw_arch_attrs = unsafe { region.arch_attrs.raw };

        Some((level, level_indices, current_addr, raw_arch_attrs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern crate std;
    use std::vec;
    use std::vec::Vec;

    use crate::shared_types::MmuRegionArchAttrs as RegionArchAttrs;

    #[test]
    fn test_bits_range() {
        assert_eq!(bits_of_range(0b0110, 1, 3), 0b11);
        assert_eq!(bits_of_range(0b1001, 1, 3), 0b00);
        assert_eq!(bits_of_range(usize::MAX, 0, usize::BITS), usize::MAX);
        assert_eq!(bits_of_range(usize::MAX, 4, usize::BITS), usize::MAX >> 4);
    }

    #[test]
    #[should_panic]
    fn test_bits_range_not_allowed() {
        bits_of_range(0b1001, 1, 1);
    }

    #[test]
    #[should_panic]
    fn test_bits_range_not_allowed2() {
        bits_of_range(0b1001, 4, 3);
    }

    #[test]
    fn test_bits_level() {
        let levels = [(32, 16), (24, 8), (12, 12), (8, 4)];
        assert_eq!(indices_of_level(&levels, 0, 10 << 32), 10);
        assert_eq!(indices_of_level(&levels, 1, 10 << 24), 10);
        assert_eq!(indices_of_level(&levels, 2, 10 << 12), 10);
        assert_eq!(indices_of_level(&levels, 3, 10 << 8), 10);
    }

    #[test]
    #[should_panic]
    fn test_invalid_range_aligned_regions() {
        let iter = [Region::EMPTY; 5].iter();
        // Should be reverse
        AlignedRegionsIter::new(iter, [(8, 4), (12, 12), (24, 8), (32, 16)]);
    }

    #[test]
    fn test_ok_range_aligned_regions() {
        let iter = [Region::EMPTY; 5].iter();
        AlignedRegionsIter::new(iter, [(32, 16), (24, 8), (12, 12), (8, 4)]);
    }

    #[test]
    fn test_iter_regions_simple() {
        let aarch64_levels = [(39, 9), (30, 9), (21, 9), (12, 9)];
        let mut iter = AlignedRegionsIter::new(
            // This should give us 4 4k regions
            [Region {
                start: 0,
                top: 0x3fff,
                arch_attrs: RegionArchAttrs { is_ram: false },
            }]
            .iter(),
            aarch64_levels,
        );

        let indices: Vec<_> = iter.collect();

        assert_eq!(
            indices,
            vec![
                (3, [0, 0, 0, 0], 0x0000, 0x0),
                (3, [0, 0, 0, 1], 0x1000, 0x0),
                (3, [0, 0, 0, 2], 0x2000, 0x0),
                (3, [0, 0, 0, 3], 0x3000, 0x0),
            ]
        );
    }

    #[test]
    fn test_iter_regions_multi_layer() {
        let aarch64_levels = [(39, 9), (30, 9), (21, 9), (12, 9)];
        let mut iter = AlignedRegionsIter::new(
            [
                // This should give us 4 4k regions
                Region {
                    start: 0,
                    top: 0x3fff,
                    arch_attrs: RegionArchAttrs { is_ram: false },
                },
                // Then this will give us 2 2M region and then 4 4k regions
                Region {
                    start: 0x200000,
                    top: 0x603fff,
                    arch_attrs: RegionArchAttrs { is_ram: true },
                },
            ]
            .iter(),
            aarch64_levels,
        );

        let indices: Vec<_> = iter.collect();

        assert_eq!(
            indices,
            vec![
                (3, [0, 0, 0, 0], 0x0000, 0x0),
                (3, [0, 0, 0, 1], 0x1000, 0x0),
                (3, [0, 0, 0, 2], 0x2000, 0x0),
                (3, [0, 0, 0, 3], 0x3000, 0x0),
                (2, [0, 0, 1, 0], 0x200000, 0x1),
                (2, [0, 0, 2, 0], 0x400000, 0x1),
                (3, [0, 0, 3, 0], 0x600000, 0x1),
                (3, [0, 0, 3, 1], 0x601000, 0x1),
                (3, [0, 0, 3, 2], 0x602000, 0x1),
                (3, [0, 0, 3, 3], 0x603000, 0x1),
            ]
        );
    }
}
