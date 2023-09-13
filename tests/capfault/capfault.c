/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

#define ECHO_CH 2

volatile uint64_t *shared_counter = (uint64_t *)(uintptr_t)0x1800000;

void
init(void)
{
    microkit_dbg_puts("capfault: forcing a cap fault by notifying invalid channel 10,000\n");
    microkit_notify(10000);
}

void
notified(microkit_channel ch)
{
}