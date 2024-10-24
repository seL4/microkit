/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

static inline void serial_putc(char ch)
{
    /* The I/O port capability and address defined here must match the system
     * description file (hello.system). */
    int cap = microkit_ioport_cap(0);
    uint16_t base = 0x3f8;

    while ((seL4_X86_IOPort_In8(cap, base + 5).result & 0x20) == 0)
        ;
    seL4_X86_IOPort_Out8(cap, base, ch);
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
	(void) ch;
}
