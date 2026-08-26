//
// Copyright 2026, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

//! These types are shared between the loader Rust code, the loader C code,
//! and the Microkit Tool's build-time code.

use core::fmt;

/// Note that "0" is a valid return value; instead the invalid value is
/// '-1', or usize::MAX.
#[repr(C)]
#[derive(Debug)]
pub struct AArch64ReturnValue {
    pub ttbr0_el2: *const u8,
    pub ttbr0_el1: *const u8,
    pub ttbr1_el1: *const u8,
}

impl AArch64ReturnValue {
    pub const INVALID: *const u8 = usize::MAX as *const _;
}

/// IMPORTANT: Keep in sync with C's `union MmuRegionArchAttrs`
#[derive(Copy, Clone)]
#[repr(C)]
pub union MmuRegionArchAttrs {
    pub is_ram: bool,
    pub raw: u64,
}

impl fmt::Debug for MmuRegionArchAttrs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("MmuRegionArchAttrs")
            // SAFETY: raw contains all valid bitpatterns
            .field("raw", unsafe { &self.raw })
            .finish()
    }
}

/// Region is [start, end] *inclusive* as this avoids overflows.
/// IMPORTANT: Keep in sync with C's `struct Region`
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct MmuRegion {
    pub start: usize,
    pub top: usize,
    pub arch_attrs: MmuRegionArchAttrs,
}

impl MmuRegion {
    pub const EMPTY: Self = Self {
        start: 0,
        top: 0,
        arch_attrs: MmuRegionArchAttrs { raw: 0 },
    };
}
