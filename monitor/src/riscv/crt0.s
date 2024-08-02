/*
 * Copyright 2024, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

.extern main
.extern __global_pointer$

.section ".text.start"

.global _start;
.type _start, %function;
_start:
.option push
.option norelax
1:  auipc gp, %pcrel_hi(__global_pointer$)
    addi  gp, gp, %pcrel_lo(1b)
.option pop
    la s1, (_stack + 0xff0)
    mv sp, s1
    j main
