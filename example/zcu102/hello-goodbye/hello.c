/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <sel4cp.h>

#define GOODBYE 0

void
init(void)
{
    sel4cp_dbg_puts("hello\n");
    sel4cp_notify(GOODBYE);
}

void
notified(sel4cp_channel ch)
{
}