/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2022, UNSW (ABN 57 195 873 179)
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

void
init(void)
{
    microkit_dbg_puts("hello, world\n");
}

void
notified(microkit_channel ch)
{
}