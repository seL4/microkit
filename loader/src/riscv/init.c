/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include "../arch.h"
#include "../cpus.h"
#include "../loader.h"
#include "../uart.h"

#include <kernel/gen_config.h>

void arch_init(void)
{
    puts("LDR|INFO: configured with FIRST_HART_ID ");
    puthex32(CONFIG_FIRST_HART_ID);
    puts("\n");
}

typedef void (*sel4_entry)(
    uintptr_t ui_p_reg_start,
    uintptr_t ui_p_reg_end,
    intptr_t pv_offset,
    uintptr_t v_entry,
    uintptr_t dtb_addr_p,
    uintptr_t dtb_size
#if defined(CONFIG_ENABLE_SMP_SUPPORT)
    ,
    uint64_t hart_id,
    uint64_t core_id
#endif
);

void arch_jump_to_kernel(int logical_cpu)
{
    uint64_t hart_id = plat_get_hw_id(logical_cpu);

    ((sel4_entry)(loader_data->kernel_entry))(
        loader_data->ui_p_reg_start,
        loader_data->ui_p_reg_end,
        loader_data->pv_offset,
        loader_data->v_entry,
        0,
        0
#if defined(CONFIG_ENABLE_SMP_SUPPORT)
        ,
        hart_id,
        logical_cpu
#endif
    );
}
