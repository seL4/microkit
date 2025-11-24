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

#ifdef CONFIG_PLAT_STAR64
static const uint64_t hart_ids[4] = {0x1, 0x2, 0x3, 0x4};
#elif defined(CONFIG_PLAT_QEMU_RISCV_VIRT)
static const uint64_t hart_ids[4] = {0x0, 0x1, 0x2, 0x3};
#elif defined(CONFIG_PLAT_HIFIVE_P550)
static const uint64_t hart_ids[4] = {0x0, 0x1, 0x2, 0x3};
#else
#error "hart_ids not defined for this board"
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
    LDR_PRINT("INFO", 0, "Starting CPU ");
    puts((const char[]) {
        '0' + logical_cpu, '\0'
    });
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

    // struct sbi_ret ret = sbi_call(SBI_EXT_HSM, SBI_HSM_HART_STOP, hart_id, 0, 0, 0, 0, 0);
    // if (ret.error != SBI_SUCCESS) {
    //     LDR_PRINT("ERROR", 0, "could not stop CPU, SBI call returned: ");
    //     puts(sbi_error_as_string(ret.error));
    //     puts("\n");
    // }
    struct sbi_ret ret = sbi_call(
                             SBI_EXT_HSM,
                             SBI_HSM_HART_START,
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
