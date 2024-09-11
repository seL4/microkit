/*
 * Copyright 2023, Neutrality.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stdint.h>

static inline void memcpy(char *dst, char *src, uint32_t len)
{
    while (len--)
        *dst++ = *src++;
}

static inline uint32_t strlen(char *str)
{
    uint32_t len = 0;
    while (*str++)
        len++;
    return len;
}

static inline uint8_t in8(uint16_t port)
{
    uint8_t value;
    __asm__ __volatile__ ("inb %w1,%0":"=a" (value):"Nd" (port));
    return value;
}

static inline void out8(uint16_t port, uint8_t value)
{
    __asm__ __volatile__ ("outb %b0,%w1": :"a" (value), "Nd" (port));
}
