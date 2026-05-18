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

void el1_mmu_enable(uint64_t aarch64_pt_ttbr0_el1, uint64_t aarch64_pt_ttbr1_el1);
void el2_mmu_enable(uint64_t aarch64_pt_ttbr0_el2);

/* Pointers to the top-level paging structures */
uint64_t aarch64_pt_ttbr0_el1;
uint64_t aarch64_pt_ttbr1_el1;
uint64_t aarch64_pt_ttbr0_el2;

int arch_mmu_enable(int logical_cpu)
{
    int r;
    enum el el;
    r = ensure_correct_el(logical_cpu);
    if (r != 0) {
        return r;
    }

    LDR_PRINT("INFO", logical_cpu, "enabling MMU\n");
    el = current_el();
    if (el == EL1) {
        el1_mmu_enable(aarch64_pt_ttbr0_el1, aarch64_pt_ttbr1_el1);
    } else if (el == EL2) {
        el2_mmu_enable(aarch64_pt_ttbr0_el2);
    } else {
        LDR_PRINT("ERROR", logical_cpu, "unknown EL for MMU enable\n");
    }

    return 0;
}
