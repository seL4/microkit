/*
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

#define SERIAL_IOPORT_ID 0
#define SERIAL_IOPORT_ADDRESS 0x3f8

static inline void serial_putc(char ch)
{
    // Danger: may overflow hardware FIFO, but we are only writing a small message.
    microkit_x86_ioport_write_8(SERIAL_IOPORT_ID, SERIAL_IOPORT_ADDRESS, ch);
}

static inline void serial_puts(const char *s)
{
    while (*s) {
        if (*s == '\n') {
            serial_putc('\r');
        }
        serial_putc(*s++);
    }
}

void init(void)
{
    microkit_dbg_puts("hello, world. my name is ");
    microkit_dbg_puts(microkit_name);
    microkit_dbg_puts("\n");

    microkit_dbg_puts("Now writing to serial I/O port: ");
    serial_puts("hello!\n");
}

void notified(microkit_channel ch)
{
}
