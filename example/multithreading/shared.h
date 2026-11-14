/*
 * Copyright 2026, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#pragma once

#include <stdint.h>
#include <stdatomic.h>

#define SHARED_MEM_VADDR 0x2000000
#define NUM_INCREMENTS   10000

typedef struct {
    atomic_flag lock;
    uint32_t counter;
    uint32_t completed;
} shared_data_t;

static inline void spin_lock(atomic_flag *lock)
{
    while (atomic_flag_test_and_set_explicit(lock, memory_order_acquire)) {
        /* busy wait */
    }
}

static inline void spin_unlock(atomic_flag *lock)
{
    atomic_flag_clear_explicit(lock, memory_order_release);
}
