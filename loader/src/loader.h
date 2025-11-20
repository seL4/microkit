/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#pragma once

#include <stdint.h>
#include <stddef.h>

#include "cpus.h"

#define REGION_TYPE_DATA 1
#define REGION_TYPE_ZERO 2

struct region {
    uintptr_t load_addr;
    uintptr_t size;
    uintptr_t offset;
    uintptr_t type;
};

struct loader_data {
    uintptr_t magic;
    uintptr_t size;
    uintptr_t kernel_entry;
    uintptr_t ui_p_reg_start;
    uintptr_t ui_p_reg_end;
    uintptr_t pv_offset;
    uintptr_t v_entry;

    uintptr_t num_regions;
    struct region regions[];
};

extern const struct loader_data *loader_data;

/* Called from assembly */
void relocation_failed(void);
void relocation_log(uint64_t reloc_addr, uint64_t curr_addr);

#define STACK_SIZE 4096

extern char _stack[NUM_ACTIVE_CPUS][STACK_SIZE];

void start_kernel(int logical_id);
