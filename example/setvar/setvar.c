/*
 * Copyright 2025, UNSW.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

uint64_t mr_with_paddr_vaddr;
uint64_t mr_with_paddr_paddr;

uint64_t small_mr_no_paddr_paddr;
uint64_t large_mr_no_paddr_paddr;

uint64_t pl011_paddr;

void init(void)
{
    microkit_dbg_puts("Virtual address of 'mr_with_paddr': ");
    microkit_dbg_put32(mr_with_paddr_vaddr);
    microkit_dbg_puts("\n");
    microkit_dbg_puts("Physical address of 'mr_with_paddr': ");
    microkit_dbg_put32(mr_with_paddr_paddr);
    microkit_dbg_puts("\n");

    microkit_dbg_puts("Physical address of 'small_mr_no_paddr': ");
    microkit_dbg_put32(small_mr_no_paddr_paddr);
    microkit_dbg_puts("\n");
    microkit_dbg_puts("Physical address of 'large_mr_no_paddr': ");
    microkit_dbg_put32(large_mr_no_paddr_paddr);
    microkit_dbg_puts("\n");

    microkit_dbg_puts("Physical address of 'pl011_paddr': ");
    microkit_dbg_put32(pl011_paddr);
    microkit_dbg_puts("\n");
}

void notified(microkit_channel ch)
{
}
