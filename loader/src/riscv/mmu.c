/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stdint.h>

#include "../arch.h"
#include "../cutil.h"

/* Paging structures for kernel mapping */
uint64_t boot_lvl1_pt[1 << 9] ALIGN(1 << 12);
uint64_t boot_lvl2_pt[1 << 9] ALIGN(1 << 12);
/* Paging structures for identity mapping */
uint64_t boot_lvl2_pt_elf[1 << 9] ALIGN(1 << 12);


/*
 * This is the encoding for the MODE field of the satp register when
 * implementing 39-bit virtual address spaces (known as Sv39).
 */
#define VM_MODE (0x8llu << 60)

#define RISCV_PGSHIFT 12

int arch_mmu_enable(int logical_cpu)
{
    // The RISC-V privileged spec (20211203), section 4.1.11 says that the
    // SFENCE.VMA instruction may need to be executed before or after writing
    // to satp. I don't understand why we do it before compared to after.
    // Need to understand 4.2.1 of the spec.
    asm volatile("sfence.vma" ::: "memory");
    asm volatile(
        "csrw satp, %0\n"
        :
        : "r"(VM_MODE | (uintptr_t)boot_lvl1_pt >> RISCV_PGSHIFT)
        :
    );
    asm volatile("fence.i" ::: "memory");

    return 0;
}
