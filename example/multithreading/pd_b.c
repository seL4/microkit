/*
 * Copyright 2026, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stdint.h>
#include <microkit.h>
#include "shared.h"

volatile shared_data_t *shared = (volatile shared_data_t *) SHARED_MEM_VADDR;

void init(void)
{
    microkit_dbg_puts(microkit_name);
    microkit_dbg_puts(": starting\n");

    for (int i = 0; i < NUM_INCREMENTS; i++) {
        spin_lock((atomic_flag *) &shared->lock);
        shared->counter++;
        spin_unlock((atomic_flag *) &shared->lock);
    }

    microkit_dbg_puts(microkit_name);
    microkit_dbg_puts(": finished increments\n");

    spin_lock((atomic_flag *) &shared->lock);
    shared->completed++;
    if (shared->completed == 2) {
        microkit_dbg_puts("All PDs finished. Final counter: ");
        microkit_dbg_put32(shared->counter);
        microkit_dbg_puts(" (expected: ");
        microkit_dbg_put32(2 * NUM_INCREMENTS);
        microkit_dbg_puts(")\nSUCCESS\n");
    }
    spin_unlock((atomic_flag *) &shared->lock);
}

void notified(microkit_channel ch)
{
}
