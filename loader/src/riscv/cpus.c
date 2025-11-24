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
#include "sbi.h"

/*
 * On RISC-V the hart IDs represent any hardware thread in the system, including
 * ones that we do not intend to run on.
 * Typically the main CPU may have something like 4 cores that we intend to run
 * seL4 on but an additional S-mode only monitor core, this is the case on certain
 * CPUs such as the SiFive U74. The problematic part is that the monitor core has
 * a hart ID of zero and hart IDs are not guaranteed to be contiguous.
 * This is why we must explitly list all the hart IDs that we want to boot on.
 * To figure this out for your platform, the best way is to look at the Device Tree,
 * each CPU will have a 'reg' field where the value is the hart ID.
 */
#if defined(CONFIG_PLAT_STAR64)
static const uint64_t hart_ids[4] = {0x1, 0x2, 0x3, 0x4};
#elif defined(CONFIG_PLAT_QEMU_RISCV_VIRT) || defined(CONFIG_PLAT_HIFIVE_P550)
static const uint64_t hart_ids[4] = {0x0, 0x1, 0x2, 0x3};
#else

_Static_assert(!is_set(CONFIG_ENABLE_SMP_SUPPORT),
               "unknown board fallback not allowed for smp targets; " \
               "please define hart_ids");

static const size_t hart_ids[1] = { CONFIG_FIRST_HART_ID };
#endif

_Static_assert(NUM_ACTIVE_CPUS <= ARRAY_SIZE(hart_ids),
               "active CPUs cannot be more than available CPUs");

void plat_save_hw_id(int logical_cpu, uint64_t hart_id)
{
    /** RISC-V appears to be nice and the hart_id given by the entrypoint
     *  should always match that of the IDs we use to start it. Here we don't
     *  need to do anything, but we can check that we are correct
     **/

    if (hart_ids[logical_cpu] != hart_id) {
        LDR_PRINT("ERROR", logical_cpu, "runtime hart id ");
        puthex64(hart_id);
        puts("does not match build-time value ");
        puthex64(hart_ids[logical_cpu]);
        puts("\n");

        for (;;) {}
    }
}

uint64_t plat_get_hw_id(int logical_cpu)
{
    return hart_ids[logical_cpu];
}

/** defined in crt0.S */
extern char riscv_secondary_cpu_entry_asm[1];
/** called from crt0.S */
void riscv_secondary_cpu_entry(uint64_t hart_id, int logical_cpu);

void riscv_secondary_cpu_entry(uint64_t hart_id, int logical_cpu)
{
    LDR_PRINT("INFO", logical_cpu, "secondary CPU entry with hart id ");
    puthex64(hart_id);
    puts("\n");

    if (logical_cpu == 0) {
        LDR_PRINT("ERROR", logical_cpu, "secondary CPU should not have logical id 0!!!\n");
        goto fail;
    } else if (logical_cpu >= NUM_ACTIVE_CPUS) {
        LDR_PRINT("ERROR", logical_cpu, "secondary CPU should not be >NUM_ACTIVE_CPUS\n");
        goto fail;
    }

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

    uint64_t *stack_base = _stack[logical_cpu];
    /* riscv expects stack to be 128-bit (16 byte) aligned, and we push to the stack
       to have space for the arguments to the entrypoint */
    uint64_t *sp = (uint64_t *)((uintptr_t)stack_base + STACK_SIZE - 2 * sizeof(uint64_t));
    /* store the logical cpu on the stack */
    sp[0] = logical_cpu;
    /* zero out what was here before */
    sp[1] = 0;

    uint64_t hart_id = plat_get_hw_id(logical_cpu);

    struct sbi_ret ret = sbi_call(
                             SBI_HSM_EID,
                             SBI_HSM_HART_START_FID,
                             /* hartid */ hart_id,
                             /* start_addr */ (uint64_t)riscv_secondary_cpu_entry_asm,
                             /* opaque */ (uint64_t)sp,
                             /* unused for this call */ 0, 0, 0);

    if (ret.error != SBI_SUCCESS) {
        LDR_PRINT("ERROR", 0, "could not start CPU, SBI call returned: ");
        puts(sbi_error_as_string(ret.error));
        puts("\n");
    }

    return 0;
}
