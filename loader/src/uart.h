/*
 * Copyright 2025, UNSW.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stdint.h>

#if PRINTING

void uart_init();
void puts(const char *s);
void puthex64(uint64_t val);
void puthex32(uint32_t val);

#else

static inline void uart_init() {}
static inline void puts(const char *s) {}
static inline void puthex64(uint64_t val) {}
static inline void puthex32(uint32_t val) {}

#endif /* PRINTING */
