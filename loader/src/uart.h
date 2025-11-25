/*
 * Copyright 2025, UNSW.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#pragma once

#include <stdint.h>

#include <kernel/gen_config.h>

#if defined(CONFIG_PRINTING)

void uart_init(void);
void puts(const char *s);
void puthex64(uint64_t val);
void puthex32(uint32_t val);
/* only 0-9 allowed */
void putdecimal(uint8_t val);

#define LDR_PRINT(lvl, cpu, msg) do {                                          \
    puts("LDR|" lvl "|CPU");                                                   \
    putdecimal(cpu);                                                           \
    puts(": " msg);                                                            \
} while (0);

#else

static inline void uart_init(void) {}
static inline void puts(const char *s) {}
static inline void puthex64(uint64_t val) {}
static inline void puthex32(uint32_t val) {}
static inline void putdecimal(uint8_t val) {}

#define LDR_PRINT(...) do { } while (0)

#endif /* CONFIG_PRINTING */
