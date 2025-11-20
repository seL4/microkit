/*
 * Copyright 2025, UNSW.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#pragma once

#include <stddef.h>
#include <stdint.h>

#include <kernel/gen_config.h>

/* Define our own variant of the seL4 config */
#define NUM_ACTIVE_CPUS CONFIG_MAX_NUM_NODES

/**
 * This is the number of cores that we are going to boot seL4 with,
 * i.e. the active configuration.
 *
 * NUM_ACTIVE_CPUS is passed by the build system and can be used where
 * compile-time constants in C are needed.
 **/
const inline int plat_get_active_cpus(void)
{
    return NUM_ACTIVE_CPUS;
}

/**
 * This is the number of cores that the platform actually has.
 **/
const int plat_get_available_cpus(void);

/**
 * Tell the platform specific code about the hardware ID corresponding
 * to the logical ID.
 * This will often be MPIDR on ARM.
 **/
void plat_save_hw_id(int logical_id, size_t hw_id);

/**
 * Start the CPU with the given logical ID.
 * Returns a non-zero integer on failure.
 **/
int plat_start_cpu(int logical_id);
