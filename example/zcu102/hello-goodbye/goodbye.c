/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <sel4cp.h>

#define HELLO 0

void
init(void)
{
}

void
notified(sel4cp_channel ch)
{
    switch (ch) {
        case HELLO:
            sel4cp_dbg_puts("goodbye\n");
            break;
    }
}