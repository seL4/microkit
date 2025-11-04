/*
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

uint64_t com1_ioport_id;
uint64_t com1_ioport_addr;

static inline void serial_putc(char ch)
{
    // Danger: may overflow hardware FIFO, but we are only writing a small message.
    microkit_x86_ioport_write_8(com1_ioport_id, com1_ioport_addr, ch);
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
