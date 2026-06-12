/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

void init(void)
{
    for (uint64_t i = 0; i < 99999999; i++) {
        if (i == 99999998) {
            microkit_dbg_puts("hello, world from ");
            microkit_dbg_puts(microkit_name);
            microkit_dbg_puts("\n");
            i = 0;
        } else {
            asm("nop");
        }
    }

}

void notified(microkit_channel ch)
{
}
