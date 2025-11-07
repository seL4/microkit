/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

void arch_set_exception_handler(void)
{
    /* Don't do anything on RISC-V since we always are in S-mode so M-mode
     * will catch our faults (e.g SBI). */
}
