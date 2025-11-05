/*
 * Copyright 2026, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

void init(void)
{
    microkit_dbg_puts("|secondary| hello, world\n");
}

void notified(microkit_channel ch)
{
    microkit_dbg_puts("|secondary| notified\n");
    microkit_notify(ch);
}
