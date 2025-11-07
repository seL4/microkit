/*
 * Copyright 2025, UNSW.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stddef.h>


#define ALIGN(n)  __attribute__((__aligned__(n)))

#define MASK(x) ((1UL << x) - 1)

void *memcpy(void *dst, const void *src, size_t sz);

void *memmove(void *restrict dest, const void *restrict src, size_t n);
