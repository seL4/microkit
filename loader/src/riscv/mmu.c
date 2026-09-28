/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stdint.h>
#include <stdbool.h>

#include "../arch.h"
#include "../cutil.h"
#include "../loader.h"

/* Pointers to the top-level paging structures */
uintptr_t riscv64_boot_lvl1_pt;

/*
 * This is the encoding for the MODE field of the satp register when
 * implementing 39-bit virtual address spaces (known as Sv39).
 */
#define VM_MODE (0x8llu << 60)

#define RISCV_PGSHIFT 12

static uint8_t page_table_bytes[PAGE_TABLE_SIZE][MAX_NUM_PAGE_TABLES] ALIGN(PAGE_TABLE_SIZE);

extern uintptr_t riscv64_setup_pagetables(
    uint64_t kernel_first_vaddr, uint64_t kernel_first_paddr,
    void *regions_ptr, uintptr_t regions_len,
    uint8_t page_table_bytes[PAGE_TABLE_SIZE][MAX_NUM_PAGE_TABLES]);

int arch_mmu_enable(int logical_cpu)
{
    struct MmuRegion *mmu_regions = get_loader_array(mmu_regions);

    uintptr_t riscv64_boot_lvl1_pt = riscv64_setup_pagetables(
        loader_data->kernel_first_vaddr, loader_data->kernel_first_paddr,
        mmu_regions, loader_data->mmu_regions_count,
        page_table_bytes
    );

    // The RISC-V privileged spec (20211203), section 4.1.11 says that the
    // SFENCE.VMA instruction may need to be executed before or after writing
    // to satp. I don't understand why we do it before compared to after.
    // Need to understand 4.2.1 of the spec.
    asm volatile("sfence.vma" ::: "memory");
    asm volatile(
        "csrw satp, %0\n"
        :
        : "r"(VM_MODE | riscv64_boot_lvl1_pt >> RISCV_PGSHIFT)
        :
    );
    asm volatile("fence.i" ::: "memory");

    return 0;
}
