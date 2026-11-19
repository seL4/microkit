/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#pragma once

#define STACK_SIZE 4096

#define REGION_TYPE_DATA 1
#define REGION_TYPE_ZERO 2

#ifndef __ASSEMBLER__

#include <stdint.h>
#include <stddef.h>

#include "cpus.h"

// Keep in sync with Rust's 'LoaderRegion64'
struct loader_region {
    uintptr_t load_addr;
    uintptr_t size;
    uintptr_t offset;
    uintptr_t type;
};

// Keep in sync with Rust's 'LoaderHeader64'
struct loader_header {
    uintptr_t magic;
    uintptr_t size;
    uintptr_t kernel_entry;
    uintptr_t ui_p_reg_start;
    uintptr_t ui_p_reg_end;
    uintptr_t pv_offset;
    uintptr_t v_entry;
    uintptr_t kernel_first_vaddr;
    uintptr_t kernel_first_paddr;

    // Offset from start of loader_header to start of loader metadata regions
    uintptr_t loader_regions_offset;
    uintptr_t loader_regions_count;

    // Offset from start of loader_header to start of mmu regions
    uintptr_t mmu_regions_offset;
    uintptr_t mmu_regions_count;
};

extern const struct loader_header *loader_data;

#define get_loader_array(name) \
    (void *)((uintptr_t)(loader_data) + loader_data-> name##_offset);

/* Called from assembly */
void relocation_failed(void);
void relocation_log(uint64_t reloc_addr, uint64_t curr_addr);

extern uint64_t _stack[NUM_ACTIVE_CPUS][STACK_SIZE / sizeof(uint64_t)];

/* Fatal error, call to not continue execution of the loader. */
void fail(void);

void start_kernel(int logical_cpu);

#endif
