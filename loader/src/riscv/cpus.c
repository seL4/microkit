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
int logical_to_hart_id[4] = {0x1, 0x2, 0x3, 0x4};
#elif defined(CONFIG_PLAT_QEMU_RISCV_VIRT)
int logical_to_hart_id[4] = {0x0, 0x1, 0x2, 0x3};
#elif defined(CONFIG_PLAT_HIFIVE_P550)
int logical_to_hart_id[4] = {0x0, 0x1, 0x2, 0x3};
#else
#error "Unsupported platform TODO"
#endif

int hart_id_to_logical(int hart_id);
void riscv_secondary_cpu_entry(int hart_id);

int hart_id_to_logical(int hart_id)
{
    for (int i = 0; i < sizeof(logical_to_hart_id) / sizeof(int); i++) {
        if (hart_id == logical_to_hart_id[i]) {
            return i;
        }
    }

    return -1;
}

/** defined in crt0.S */
extern char riscv_secondary_cpu_entry_asm[1];

void riscv_secondary_cpu_entry(int hart_id)
{
    int logical_cpu = hart_id_to_logical(hart_id);
    if (logical_cpu == -1) {
        LDR_PRINT("ERROR", logical_cpu, "invalid hart ID\n");
        goto fail;
    }
    // TODO: print hart_id and check logical cpu is not -1
    LDR_PRINT("INFO", logical_cpu, "secondary CPU entry\n");

    if (logical_cpu == 0) {
        LDR_PRINT("ERROR", logical_cpu, "secondary CPU should not have loader id 0!!!\n");
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

    uint64_t stack_base = (uint64_t)&_stack[logical_cpu][0];
    uint64_t stack_top = stack_base + STACK_SIZE;
    uint64_t sp = stack_top;

    int hart_id = logical_to_hart_id[logical_cpu];
    // struct sbi_ret ret = sbi_call(SBI_EXT_HSM, SBI_HSM_HART_STOP, hart_id, 0, 0, 0, 0, 0);
    // if (ret.error != SBI_SUCCESS) {
    //     LDR_PRINT("ERROR", 0, "could not stop CPU, SBI call returned: ");
    //     puts(sbi_error_as_string(ret.error));
    //     puts("\n");
    // }
    struct sbi_ret ret = sbi_call(SBI_EXT_HSM, SBI_HSM_HART_START, hart_id, (uint64_t)riscv_secondary_cpu_entry_asm, sp, 0,
                                  0, 0);

    if (ret.error != SBI_SUCCESS) {
        LDR_PRINT("ERROR", 0, "could not start CPU, SBI call returned: ");
        puts(sbi_error_as_string(ret.error));
        puts("\n");
    }

    return 0;
}
