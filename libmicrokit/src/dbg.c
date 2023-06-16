/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <microkit.h>

#define __thread
#include <sel4/sel4.h>

void
microkit_dbg_putc(int c)
{
#if defined(CONFIG_PRINTING)
    seL4_DebugPutChar(c);
#endif
}



void
microkit_dbg_puts(const char *s)
{
    while (*s) {
        microkit_dbg_putc(*s);
        s++;
    }
}


void
__assert_fail(const char  *str, const char *file, int line, const char *function)
{
    microkit_dbg_puts("assert failed: ");
    microkit_dbg_puts(str);
    microkit_dbg_puts(" ");
    microkit_dbg_puts(file);
    microkit_dbg_puts(" ");
    microkit_dbg_puts(function);
    microkit_dbg_puts("\n");
}
