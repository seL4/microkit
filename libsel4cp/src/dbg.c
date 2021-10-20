/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <sel4cp.h>

#define __thread
#include <sel4/sel4.h>

void
sel4cp_dbg_putc(int c)
{
#if defined(CONFIG_DEBUG_BUILD)
    seL4_DebugPutChar(c);
#endif
}



void
sel4cp_dbg_puts(const char *s)
{
    while (*s) {
        sel4cp_dbg_putc(*s);
        s++;
    }
}


void
__assert_fail(const char  *str, const char *file, int line, const char *function)
{
    sel4cp_dbg_puts("assert failed: ");
    sel4cp_dbg_puts(str);
    sel4cp_dbg_puts(" ");
    sel4cp_dbg_puts(file);
    sel4cp_dbg_puts(" ");
    sel4cp_dbg_puts(function);
    sel4cp_dbg_puts("\n");
}
