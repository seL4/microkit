/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#pragma once

#include <stdint.h>
#include <sel4/sel4.h>

void putc(uint8_t ch);
void puts(const char *s);
void puthex32(uint32_t val);
void puthex64(uint64_t val);
void fail(char *s);
char* sel4_strerror(seL4_Word err);