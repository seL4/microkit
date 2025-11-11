/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include "../uart.h"

#include <kernel/gen_config.h>

void arch_init(void)
{
    puts("LDR|INFO: configured with FIRST_HART_ID ");
    puthex32(CONFIG_FIRST_HART_ID);
    puts("\n");
}
