/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <microkit.h>

#include <sel4/sel4.h>

extern char microkit_name[];

void microkit_dbg_putc(int c)
{
#if defined(CONFIG_PRINTING)
    seL4_DebugPutChar(c);
#endif
}



void microkit_dbg_puts(const char *s)
{
    while (*s) {
        microkit_dbg_putc(*s);
        s++;
    }
}

void microkit_dbg_put8(seL4_Uint8 x)
{
    char tmp[4];
    unsigned i = 3;
    tmp[3] = 0;
    do {
        seL4_Uint8 c = x % 10;
        tmp[--i] = '0' + c;
        x /= 10;
    } while (x);
    microkit_dbg_puts(&tmp[i]);
}

void microkit_dbg_put32(seL4_Uint32 x)
{
    char tmp[11];
    unsigned i = 10;
    tmp[10] = 0;
    do {
        seL4_Uint8 c = x % 10;
        tmp[--i] = '0' + c;
        x /= 10;
    } while (x);
    microkit_dbg_puts(&tmp[i]);
}

/*
 * We have to provide an implementation for libsel4 debug asserts, make it
 * weak so users can override with their own libc etc.
 */
__attribute__((weak)) void __assert_fail(const char  *str, const char *file, int line, const char *function)
{
    microkit_dbg_puts(microkit_name);
    microkit_dbg_puts("|assert failed: ");
    microkit_dbg_puts(str);
    microkit_dbg_puts(" ");
    microkit_dbg_puts(file);
    microkit_dbg_puts(":");
    microkit_dbg_put32(line);
    microkit_dbg_puts(" ");
    microkit_dbg_puts(function);
    microkit_dbg_puts("\n");
    microkit_internal_crash(0);
}
