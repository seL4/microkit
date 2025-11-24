/*
 * Copyright 2025, UNSW.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stddef.h>
#include <stdint.h>

#include "../cpus.h"
#include "../cutil.h"
#include "../loader.h"
#include "../uart.h"

void arm_secondary_cpu_entry(int logical_cpu, uint64_t mpidr_el1);

/**
 * For the moment this code assumes that CPUs are booted using the ARM PSCI
 * standard. We reference Version 1.3 issue F.b.
 **/

size_t cpu_mpidrs[NUM_ACTIVE_CPUS];

void plat_save_hw_id(int logical_cpu, size_t hw_id)
{
    cpu_mpidrs[logical_cpu] = hw_id;
}

uint64_t plat_get_hw_id(int logical_cpu)
{
    return cpu_mpidrs[logical_cpu];
}

/**
 * This is the 'target_cpu' of the CPU_ON, which is *supposed* to be the MPIDR
 * value, but is not always (e.g. in the ODROID boards). This value is derived
 * from the device tree (cpu's <reg> argument), which is what Linux uses.
 **/

#if defined(CONFIG_PLAT_MAAXBOARD)
static const size_t psci_target_cpus[4] = {0x00, 0x01, 0x02, 0x03};
#elif defined(CONFIG_PLAT_ODROIDC4)
static const size_t psci_target_cpus[4] = {0x00, 0x01, 0x02, 0x03};
#elif defined(CONFIG_PLAT_QEMU_ARM_VIRT)
/* QEMU is special and can have arbitrary numbers of cores */
// TODO.
static const size_t psci_target_cpus[4] = {0x00, 0x01, 0x02, 0x03};
#else

_Static_assert(!is_set(CONFIG_ENABLE_SMP_SUPPORT),
               "unknown board fallback not allowed for smp targets; " \
               "please define psci_target_cpus");

static const size_t psci_target_cpus[1] = {0x00};
#endif

_Static_assert(NUM_ACTIVE_CPUS <= ARRAY_SIZE(psci_target_cpus),
               "active CPUs cannot be more than available CPUs");

/**
 * The Power State Coordinate Interface (DEN0022F.b) document in §5.2.1
 * specifies that
 *
 *      For PSCI functions that use only 32-bit parameters, the arguments are
 *      passed in R0 to R3 (AArch32) or W0 to W3 (AArch64), with return values
 *      in R0 or W0. For versions using 64-bit parameters, the arguments are
 *      passed in X0 to X3, with return values in X0. In line with the SMC
 *      Calling Conventions, the immediate value used with an SMC (or HVC)
 *      instruction must be 0.
 *
 * Hence we need both the SMC64 and SMC32 conventions implemented, with support
 * for up to 3 arguments.
 *
 * Table 5 Return error codes gives the error codes.
 * Errors are 32-bit signed integers for SMC32 functions, and 64-bit signed integers for SMC64 functions
 **/

#define PSCI_RETURN_SUCCESS 0
#define PSCI_RETURN_NOT_SUPPORTED -1
#define PSCI_RETURN_INVALID_PARAMETERS -2
#define PSCI_RETURN_DENIED -3
#define PSCI_RETURN_ALREADY_ON -4
#define PSCI_RETURN_ON_PENDING -5
#define PSCI_RETURN_INTERNAL_FAILURE -6
#define PSCI_RETURN_NOT_PRESENT -7
#define PSCI_RETURN_DISABLED -8
#define PSCI_RETURN_INVALID_ADDRESS -9

static inline const char *psci_return_as_string(uint32_t ret)
{
    switch (ret) {
    case PSCI_RETURN_SUCCESS:
        return "SUCCESS";
    case PSCI_RETURN_NOT_SUPPORTED:
        return "NOT_SUPPORTED";
    case PSCI_RETURN_INVALID_PARAMETERS:
        return "INVALID_PARAMETERS";
    case PSCI_RETURN_DENIED:
        return "DENIED";
    case PSCI_RETURN_ALREADY_ON:
        return "ALREADY_ON";
    case PSCI_RETURN_ON_PENDING:
        return "ON_PENDING";
    case PSCI_RETURN_INTERNAL_FAILURE:
        return "INTERNAL_FAILURE";
    case PSCI_RETURN_NOT_PRESENT:
        return "NOT_PRESENT";
    case PSCI_RETURN_DISABLED:
        return "DISABLED";
    case PSCI_RETURN_INVALID_ADDRESS:
        return "INVALID_ADDRESS";
    default:
        return "<unknown return>";
    }
}

/* §5.1.4 of PSCI */
#define PSCI_FUNCTION_CPU_ON 0xC4000003

/**
 * See document DEN002E SMC Calling Convention (v1.4, May 2022),
 * specifically §2.7 "SMC64/HVC64 argument passing".
 *
 * We only support up to 4 arguments, but the actual convention supports up
 * to 17, and clobbers X4--X17. The convention also supports up to 17 returns,
 * but we again only support 1.
 *
 **/
uint64_t arm_smc64_call(uint32_t function_id, uint64_t arg0, uint64_t arg1, uint64_t arg2)
{
    // Per Table 2-1, it should be 1 for SMC64.
    if ((function_id & BIT(30)) == 0) {
        puts("LDR|ERROR: SMC32 function used in SMC64 call\n");
        return PSCI_RETURN_INVALID_PARAMETERS;
    }

    register uint64_t x0 asm("x0") = function_id;
    register uint64_t x1 asm("x1") = arg0;
    register uint64_t x2 asm("x2") = arg1;
    register uint64_t x3 asm("x3") = arg2;
    asm volatile(
        "smc #0\n"
        : "=r"(x0)
        : "r"(x0), "r"(x1), "r"(x2), "r"(x3)
        : "x4", "x5", "x6", "x7", "x8", "x9", "x10", "x11", "x12", "x13", "x14", "x15", "x16", "x17",
        "memory"
    );

    return x0;
}

/**
 * Reference 2.6 "SMC32/HVC32 argument passing".
 **/
uint32_t arm_smc32_call(uint32_t function_id, uint32_t arg0, uint32_t arg1, uint32_t arg2)
{
    // Per Table 2-1, it should be 0 for SMC32.
    if ((function_id & BIT(30)) == BIT(30)) {
        puts("LDR|ERROR: SMC64 function used in SMC32 call\n");
        return PSCI_RETURN_INVALID_PARAMETERS;
    }
    // TODO: This only supports AArch64 mode for the moment, in AArch32 mode
    //       we need to use the x... registers.

    register uint64_t w0 asm("w0") = function_id;
    register uint64_t w1 asm("w1") = arg0;
    register uint64_t w2 asm("w2") = arg1;
    register uint64_t w3 asm("w3") = arg2;
    asm volatile(
        "smc #0\n"
        : "=r"(w0)
        : "r"(w0), "r"(w1), "r"(w2), "r"(w3)
        : "w4", "w5", "w6", "w7", "w8", "w9", "w10", "w11", "w12", "w13", "w14", "w15", "w16", "w17",
        "memory"
    );

    return w0;
}

/** defined in util64.S */
extern void arm_secondary_cpu_entry_asm(void *sp);

void arm_secondary_cpu_entry(int logical_cpu, uint64_t mpidr_el1)
{
    LDR_PRINT("INFO", logical_cpu, "secondary CPU entry with MPIDR_EL1 ");
    puthex64(mpidr_el1);
    puts("\n");

    if (logical_cpu == 0) {
        LDR_PRINT("ERROR", logical_cpu, "secondary CPU should not have logical id 0!!!\n");
        goto fail;
    } else if (logical_cpu >= NUM_ACTIVE_CPUS) {
        LDR_PRINT("ERROR", logical_cpu, "secondary CPU should not be >NUM_ACTIVE_CPUS\n");
        goto fail;
    } else if (logical_cpu < 0) {
        LDR_PRINT("ERROR", logical_cpu, "secondary CPU should not have negative logical id\n");
        goto fail;
    }

    plat_save_hw_id(logical_cpu, mpidr_el1);

    start_kernel(logical_cpu);

fail:
    for (;;) {}
}

int plat_start_cpu(int logical_cpu)
{
    LDR_PRINT("INFO", 0, "Starting CPU ");
    puts((const char[]) {
        '0' + logical_cpu, '\0'
    });
    puts("\n");

    if (logical_cpu >= NUM_ACTIVE_CPUS) {
        LDR_PRINT("ERROR", 0, "starting a CPU with number above the active CPU count\n");
        return 1;
    }

    /**
     * In correspondence with what arm_secondary_cpu_entry does, we push
     * some useful information to the stack.
     **/
    uint64_t *stack_base = _stack[logical_cpu];
    /* aarch64 expects stack to be 16-byte aligned, and we push to the stack
       to have space for the arguments to the entrypoint */
    uint64_t *sp = (uint64_t *)((uintptr_t)stack_base + STACK_SIZE - 2 * sizeof(uint64_t));
    /* store the logical cpu on the stack */
    sp[0] = logical_cpu;
    /* zero out what was here before */
    sp[1] = 0;

    /* Arguments as per 5.1.4 CPU_ON of the PSCI spec.

       §5.6 CPU_ON and §6.4 describes that:

       - the entry_point_address must be the physical address
       - the PSCI implementation handles cache invalidation and coherency
       - context_id is passed in the x0 register
    */
    uint64_t ret = arm_smc64_call(
                       PSCI_FUNCTION_CPU_ON,
                       /* target_cpu */ psci_target_cpus[logical_cpu],
                       /* entry_point_address */ (uint64_t)arm_secondary_cpu_entry_asm,
                       /* context_id */ (uint64_t)sp
                   );

    if (ret != PSCI_RETURN_SUCCESS) {
        LDR_PRINT("ERROR", 0, "could not start CPU, PSCI returned: ");
        puts(psci_return_as_string(ret));
        puts("\n");
    }

    return ret;
}
