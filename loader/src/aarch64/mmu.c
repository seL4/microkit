/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stdint.h>

#include "el.h"
#include "../arch.h"
#include "../cutil.h"
#include "../uart.h"

void el1_mmu_enable(void);
void el2_mmu_enable(void);

/* Paging structures for kernel mapping */
uint64_t boot_lvl0_upper[1 << 9] ALIGN(1 << 12);
uint64_t boot_lvl1_upper[1 << 9] ALIGN(1 << 12);
uint64_t boot_lvl2_upper[1 << 9] ALIGN(1 << 12);

/* Paging structures for identity mapping */
uint64_t boot_lvl0_lower[1 << 9] ALIGN(1 << 12);
uint64_t boot_lvl1_lower[1 << 9] ALIGN(1 << 12);

int arch_mmu_enable(void)
{
    int r;
    enum el el;
    r = ensure_correct_el();
    if (r != 0) {
        return r;
    }

    puts("LDR|INFO: enabling MMU\n");
    el = current_el();
    if (el == EL1) {
        el1_mmu_enable();
    } else if (el == EL2) {
        el2_mmu_enable();
    } else {
        puts("LDR|ERROR: unknown EL level for MMU enable\n");
    }

    return 0;
}
