/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include "../arch.h"
#include "../cutil.h"
#include "../uart.h"

#if __riscv_xlen == 32
#define SCAUSE_IRQ BIT(31)
#define SCAUSE_CODE MASK(31)
#else
#define SCAUSE_IRQ BIT(63)
#define SCAUSE_CODE MASK(63)
#endif

static inline uint64_t read_stval(void)
{
    uint64_t temp;
    asm volatile("csrr %0, stval" : "=r"(temp));
    return temp;
}

static inline uint64_t read_scause(void)
{
    uint64_t temp;
    asm volatile("csrr %0, scause" : "=r"(temp));
    return temp;
}

static inline uint64_t read_sepc(void)
{
    uint64_t temp;
    asm volatile("csrr %0, sepc" : "=r"(temp));
    return temp;
}

static inline void write_stvec(void *value)
{
    asm volatile("csrw stvec, %0" :: "rK"(value) : "memory");
}

static char *scause_to_string(uint64_t scause)
{
    if (scause & SCAUSE_IRQ) {
        switch (scause & SCAUSE_CODE) {
        case 1:
            return "Supervisor software interrupt";
        case 5:
            return "Supervisor timer interrupt";
        case 9:
            return "Supervisor external interrupt";
        case 13:
            return "Counter-overflow interrupt";
        default:
            return "<unknown>";
        }
    } else {
        switch (scause) {
        case 0:
            return "Instruction address misaligned";
        case 1:
            return "Instruction access fault";
        case 2:
            return "Illegal instruction";
        case 3:
            return "Breakpoint";
        case 4:
            return "Load address misaligned";
        case 5:
            return "Load access fault";
        case 6:
            return "Store/AMO address misaligned";
        case 7:
            return "Store/AMO access fault";
        case 8:
            return "Environment call from U-mode";
        case 9:
            return "Environment call from S-mode";
        case 12:
            return "Instruction page fault";
        case 13:
            return "Load page fault";
        case 15:
            return "Store/AMO page fault";
        case 18:
            return "Software check";
        case 19:
            return "Hardware error";
        default:
            return "<unknown>";
        }
    }
}

static void exception_handler(void)
{
    uint64_t scause = read_scause();
    puts("LDR|ERROR: loader trapped exception with reason '");
    puts(scause_to_string(scause));
    puts("' (scause: ");
    puthex64(scause);
    puts(", stval: ");
    puthex64(read_stval());
    puts(", sepc: ");
    puthex64(read_sepc());
    puts(")\n");

    while (1) {}
}

void arch_set_exception_handler(void)
{
    write_stvec((void *)exception_handler);
}
