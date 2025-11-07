/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include "../uart.h"

void arch_init(void)
{
    puts("LDR|INFO: configured with FIRST_HART_ID ");
    puthex32(FIRST_HART_ID);
    puts("\n");
}
