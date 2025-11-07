/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stdint.h>
#include <stddef.h>

#define REGION_TYPE_DATA 1
#define REGION_TYPE_ZERO 2

#define FLAG_SEL4_HYP (1UL << 0)

struct region {
    uintptr_t load_addr;
    uintptr_t size;
    uintptr_t offset;
    uintptr_t type;
};

struct loader_data {
    uintptr_t magic;
    uintptr_t size;
    uintptr_t flags;
    uintptr_t kernel_entry;
    uintptr_t ui_p_reg_start;
    uintptr_t ui_p_reg_end;
    uintptr_t pv_offset;
    uintptr_t v_entry;

    uintptr_t num_regions;
    struct region regions[];
};

extern const struct loader_data *loader_data;
