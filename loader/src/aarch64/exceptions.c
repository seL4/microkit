/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include "el.h"
#include "exceptions.h"
#include "../cutil.h"
#include "../loader.h"
#include "../uart.h"

extern char arm_vector_table[1];


void arch_set_exception_handler()
{
    enum el el = current_el();
    if (el == EL2) {
        asm volatile("msr vbar_el2, %0" :: "r"(arm_vector_table));
    }
    /* Since we call the exception handler before we check we're at
     * a valid EL we shouldn't assume we are at EL1 or higher. */
    if (el != EL0) {
        asm volatile("msr vbar_el1, %0" :: "r"(arm_vector_table));
    }
}

uintptr_t exception_register_state[32];

void exception_handler(uintptr_t ex)
{
    /* Read ESR/FSR based on the exception level we're at. */
    uint64_t esr;
    uintptr_t far;

    if (loader_data->flags & FLAG_SEL4_HYP) {
        asm volatile("mrs %0, ESR_EL2" : "=r"(esr) :: "cc");
        asm volatile("mrs %0, FAR_EL2" : "=r"(far) :: "cc");
    } else {
        asm volatile("mrs %0, ESR_EL1" : "=r"(esr) :: "cc");
        asm volatile("mrs %0, FAR_EL1" : "=r"(far) :: "cc");
    }

    uintptr_t ec = (esr >> 26) & 0x3f;
    puts("\nLDR|ERROR: loader trapped exception: ");
    puts(ex_to_string(ex));
    if (loader_data->flags & FLAG_SEL4_HYP) {
        puts("\n    esr_el2: ");
    } else {
        puts("\n    esr_el1: ");
    }
    puthex64(esr);
    puts("\n    ec: ");
    puthex32(ec);
    puts(" (");
    puts(ec_to_string(ec));
    puts(")\n    il: ");
    puthex64((esr >> 25) & 1);
    puts("\n    iss: ");
    puthex64(esr & MASK(24));
    puts("\n    far: ");
    puthex64(far);
    puts("\n");

    for (unsigned i = 0; i < 32; i++)  {
        puts("    reg: ");
        puthex32(i);
        puts(": ");
        puthex64(exception_register_state[i]);
        puts("\n");
    }

    for (;;) {
    }
}
