/*
 * Copyright 2025, UNSW.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stddef.h>
#include <stdint.h>

#include "smc.h"
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
    LDR_PRINT("INFO", 0, "starting CPU ");
    putdecimal(logical_cpu);
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
