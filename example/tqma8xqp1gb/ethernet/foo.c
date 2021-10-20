/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <sel4cp.h>

#define ECHO_CH 2

volatile uint64_t *shared_counter = (uint64_t *)(uintptr_t)0x1800000;

void
init(void)
{
    sel4cp_dbg_puts("foo: foo protection domain init function running\n");
    sel4cp_dbg_puts("foo: sending a notification\n");
    *shared_counter = 0x37;
    sel4cp_notify(ECHO_CH);
    sel4cp_dbg_puts("foo: sent a notification\n");
}

void
notified(sel4cp_channel ch)
{
    switch (ch) {
        case ECHO_CH:
            sel4cp_dbg_puts("foo: received notification on echo channel\n");
            if (*shared_counter == 0x38) {
                sel4cp_dbg_puts("foo: counter is expected value\n");
            } else {
                sel4cp_dbg_puts("foo: counter is unexpected value\n");
            }

            break;

        default:
            sel4cp_dbg_puts("foo: received notification on unexpected channel\n");
            break;
        /* ignore any other channels */
    }
}