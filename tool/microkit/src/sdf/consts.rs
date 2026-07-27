//
// Copyright 2025, UNSW
//
// SPDX-License-Identifier: BSD-2-Clause
//

/// Events that come through entry points (e.g notified or protected) are given an
/// identifier that is used as the badge at runtime.
/// On 64-bit platforms, this badge has a limit of 64-bits which means that we are
/// limited in how many IDs a Microkit protection domain has since each ID represents
/// a unique bit.
/// Currently the first bit is used to determine whether or not the event is a PPC
/// or notification. The second bit is used to determine whether a fault occurred.
/// This means we are left with 62 bits for the ID.
/// IDs start at zero.
pub const PD_MAX_ID: u64 = 61;
pub const VCPU_MAX_ID: u64 = PD_MAX_ID;

/// This is the maximum slot allowed for cap maps. This can change if you wish,
/// but also update the MICROKIT_MAX_USER_CAPS define in `microkit.h`.
pub const CAP_MAP_MAX_SLOT: u64 = 128;

pub const MONITOR_PRIORITY: u8 = 255;
pub const PD_MAX_PRIORITY: u8 = 254;
/// In microseconds
pub const BUDGET_DEFAULT: u64 = 1000;

pub const MONITOR_PD_NAME: &str = "monitor";
pub const MONITOR_DOMAIN: u8 = 0;

/// Default to a stack size of 8KiB
pub const PD_DEFAULT_STACK_SIZE: u64 = 0x2000;
pub const PD_MIN_STACK_SIZE: u64 = 0x1000;
pub const PD_MAX_STACK_SIZE: u64 = 1024 * 1024 * 16;

/// Maximum x86 IRQ vector value. Inclusive.
/// This value is calculated by the kernel as `irq_user_max - irq_user_min` in
/// `src/arch/x86/object/interrupt.c`
pub const X86_IRQ_VECTOR_MAX: i64 = 107;
