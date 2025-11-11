/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include "el.h"

#include <kernel/gen_config.h>

#include "../cutil.h"
#include "../loader.h"
#include "../uart.h"

void switch_to_el1(void);
void switch_to_el2(void);

/* Returns the current exception level */
enum el current_el(void)
{
    /* See: C5.2.1 CurrentEL */
    uint32_t val;
    asm volatile("mrs %x0, CurrentEL" : "=r"(val) :: "cc");
    /* bottom two bits are res0 */
    return (enum el) val >> 2;
}


int ensure_correct_el(void)
{
    enum el el = current_el();

    puts("LDR|INFO: CurrentEL=");
    puts(el_to_string(el));
    puts("\n");

    if (el == EL0) {
        puts("LDR|ERROR: Unsupported initial exception level\n");
        return 1;
    }

    if (el == EL3) {
        puts("LDR|INFO: Dropping from EL3 to EL2(NS)\n");
        switch_to_el2();
        puts("LDR|INFO: Dropped from EL3 to EL2(NS)\n");
        el = EL2;
    }

    if (is_set(CONFIG_ARM_HYPERVISOR_SUPPORT)) {
        if (el != EL2) {
            puts("LDR|ERROR: seL4 configured as a hypervisor, but not in EL2\n");
            return 1;
        } else {
            puts("LDR|INFO: Resetting CNTVOFF\n");
            asm volatile("msr cntvoff_el2, xzr");
        }
    } else {
        if (el == EL2) {
            /* seL4 relies on the timer to be set to a useful value */
            puts("LDR|INFO: Resetting CNTVOFF\n");
            asm volatile("msr cntvoff_el2, xzr");
            puts("LDR|INFO: Dropping from EL2 to EL1\n");
            switch_to_el1();
            puts("LDR|INFO: CurrentEL=");
            el = current_el();
            puts(el_to_string(el));
            puts("\n");
            if (el == EL1) {
                puts("LDR|INFO: Dropped to EL1 successfully\n");
            } else {
                puts("LDR|ERROR: Failed to switch to EL1\n");
                return 1;
            }
        }
    }

    return 0;
}
