/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <sel4cp.h>

void
init(void)
{
    int *x = 0;
    sel4cp_dbg_puts("crasher, starting\n");
    /* Crash! */
    *x = 1;
}

void
notified(sel4cp_channel ch)
{
}