/*
 * Copyright 2025, UNSW.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

#define PSCI_VERSION_FID 0x84000000

void init(void)
{
    microkit_dbg_puts("Getting SMC version via microkit_arm_smc_call()\n");

    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext resp = {0};

    args.x0 = PSCI_VERSION_FID;
    microkit_arm_smc_call(&args, &resp);

    microkit_dbg_puts("PSCI version: ");
    microkit_dbg_put32(((uint32_t) resp.x0 >> 16) & 0xFFFF);
    microkit_dbg_puts(".");
    microkit_dbg_put32((uint32_t) resp.x0 & 0xFFFF);
    microkit_dbg_puts("\n");
}

void notified(microkit_channel ch)
{
}
