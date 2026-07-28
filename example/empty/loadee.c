/*
 * Copyright 2026, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

void init(void)
{
    microkit_dbg_puts("### loadee, starting\n");
    for (int i = 0; i < 300000; ++i) {
        __asm__ volatile("nop");
    }
    int *x = 0;
    /* crash here... */
    *x = 1;
}

void notified(microkit_channel ch)
{
}