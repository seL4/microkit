/*
 * Copyright 2025, UNSW.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#pragma once

/**
  * The layout and naming scheme of the functions in these files has meaning:
  *
  * -   Functions that start with `arch_`, as specified within `arch.h`, e.g.
  *     `arch_mmu_enable`, conform to a common interface across all architectures.
  *     Code for these functions belongs in the architecture-specific subdirectory.
  *
  * -   Functions that start with `plat_` are platform/board specific, conforming
  *     to a common interface. This is useful for code such as CPU on/offlining.
  *     This needs to be called from common code, but has details that are arch
  *     and/or platform specific.
  *
  * -   Functions starting with `arm_` or `riscv_` correspond to architecture-specific
  *     code that is *not* expected to conform to a common interface. For instance
  *     `arm_secondary_cpu_entry` is prefixed as such because it needs to do ARM
  *     specific functions and is called by an ARM-specific convention.
  *
  * -   Remaining functions are somewhat of a grab bag; some code in the non-arch-
  *     specific files are for code common between all architectures, but these
  *     also contain architecture specific code, depending.
  *     Also, the UART code is not prefixed with `plat_` even though it conceptually
  *     could be that way, simply because it makes function calls more verbose,
  *     and UART is obviously platform specific.
 */

void arch_init(void);
void arch_set_exception_handler(void);
int arch_mmu_enable(void);
