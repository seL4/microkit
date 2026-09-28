/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stdint.h>
#include <stdbool.h>

#include "el.h"
#include "../arch.h"
#include "../cutil.h"
#include "../loader.h"
#include "../uart.h"

void el1_mmu_enable(uintptr_t ttbr0_el1, uintptr_t ttbr1_el1);
void el2_mmu_enable(uintptr_t ttbr0_el2);

struct AArch64ReturnValue {
    uintptr_t ttbr0_el2;
    uintptr_t ttbr0_el1;
    uintptr_t ttbr1_el1;
};

static uint8_t page_table_bytes[PAGE_TABLE_SIZE][MAX_NUM_PAGE_TABLES] ALIGN(PAGE_TABLE_SIZE);

extern struct AArch64ReturnValue aarch64_setup_pagetables(
    uint64_t kernel_first_vaddr, uint64_t kernel_first_paddr,
    void *regions_ptr, uintptr_t regions_len,
    uint8_t page_table_bytes[PAGE_TABLE_SIZE][MAX_NUM_PAGE_TABLES]);

int arch_mmu_enable(int logical_cpu)
{
    struct MmuRegion *mmu_regions = get_loader_array(mmu_regions);

    struct AArch64ReturnValue pt = aarch64_setup_pagetables(
        loader_data->kernel_first_vaddr, loader_data->kernel_first_paddr,
        mmu_regions, loader_data->mmu_regions_count,
        page_table_bytes
    );

    int r;
    enum el el;
    r = ensure_correct_el(logical_cpu);
    if (r != 0) {
        return r;
    }

    LDR_PRINT("INFO", logical_cpu, "enabling MMU\n");
    el = current_el();
    if (el == EL1) {
        el1_mmu_enable(pt.ttbr0_el1, pt.ttbr1_el1);
    } else if (el == EL2) {
        el2_mmu_enable(pt.ttbr0_el2);
    } else {
        LDR_PRINT("ERROR", logical_cpu, "unknown EL for MMU enable\n");
    }
    LDR_PRINT("INFO", logical_cpu, "... complete\n");

    return 0;
}
