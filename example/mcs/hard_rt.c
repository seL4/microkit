/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

void do_work(void)
{
    int i = 0;
    for (;;) {
        i++;
    }
    return;
}

void init(void)
{
    microkit_dbg_puts("hard_rt, starting\n");
    do_work();
}

void notified(microkit_channel ch)
{
    do_work();
}
