/*
 * Copyright 2025, UNSW.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stddef.h>


#define ALIGN(n)  __attribute__((__aligned__(n)))

#define MASK(x) ((1UL << x) - 1)

#define is_set(macro) _is_set_(macro)
#define _macrotest_1 ,
#define _is_set_(value) _is_set__(_macrotest_##value)
#define _is_set__(comma) _is_set___(comma 1, 0)
#define _is_set___(_, v, ...) v

void *memcpy(void *dst, const void *src, size_t sz);

void *memmove(void *restrict dest, const void *restrict src, size_t n);
