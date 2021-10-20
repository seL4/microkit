/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <sel4cp.h>

volatile uint8_t *p;
volatile uint8_t *q;

void
init(void)
{
    *p = 'A';
    *q = 'Z';
}

void
notified(sel4cp_channel ch)
{
}