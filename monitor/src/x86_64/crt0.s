/*
 * Copyright 2024, Neutrality Sarl.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

    .section .text.start
    .globl _start
_start:
    leaq    0xff0 + _stack(%rip), %rsp
    call    main
