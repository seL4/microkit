/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

char xxxxxxxxxxx[100000000] = { 1 };

void init(void)
{
    microkit_dbg_puts("hello, world\n");
}

void notified(microkit_channel ch)
{
}
