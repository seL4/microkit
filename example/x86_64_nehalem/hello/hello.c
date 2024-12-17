/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

static inline void serial_putc(char ch)
{
    seL4_X86_IOPort_Out8(microkit_ioport_cap(0), 0x3f8, ch);
}

static inline void serial_puts(const char *s)
{
    while (*s) {
        if (*s == '\n')
            serial_putc('\r');
        serial_putc(*s++);
    }
}

void init(void)
{
    microkit_dbg_puts("hello, debug port\n");
    serial_puts("hello, serial port\n");
}

void notified(microkit_channel ch)
{
}
