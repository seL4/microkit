/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
.extern main

.section ".text.start"

.global _start;
.type _start, %function;
_start:

    mrs    x0, mpidr_el1
    and    x0, x0,#0xFF      // Check processor id
    cbz    x0, master        // Hang for all non-primary CPU

proc_hang:
    wfe
    b proc_hang

master:
    ldr x1, =_stack + 0xff0
    mov sp, x1
    b main
