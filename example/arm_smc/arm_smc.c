/*
 * Copyright 2025, UNSW.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

#define CAP_SMC  (microkit_cspace_root_slot_to_cptr(1))
#define PSCI_VERSION_FID 0x84000000
#define PSCI_FUNCTION_CPU_ON 0x84000001

void init(void)
{
    microkit_dbg_puts("Getting SMC version via seL4_ARM_SMC_Call()\n");

    seL4_ARM_SMCContext args = { .x0 = PSCI_VERSION_FID, 0 };
    seL4_ARM_SMCContext resp = { 0 };

    seL4_Error err;
    err = seL4_ARM_SMC_Call(CAP_SMC, &args, &resp);
    if (err != seL4_NoError) {
        // Possible if you invoke with the wrong function IDs, amongst others
        microkit_dbg_puts("internal error: failed to make SMC call\n");
        return;
    }

    microkit_dbg_puts("PSCI version: ");
    microkit_dbg_put32(((uint32_t) resp.x0 >> 16) & 0xFFFF);
    microkit_dbg_puts(".");
    microkit_dbg_put32((uint32_t) resp.x0 & 0xFFFF);
    microkit_dbg_puts("\n");

    // This is not allowed!
    args.x0 = PSCI_FUNCTION_CPU_ON;

    microkit_dbg_puts("Trying to power a CPU ON (which should not work)\n");
    err = seL4_ARM_SMC_Call(CAP_SMC, &args, &resp);
    if (err == seL4_NoError) {
        microkit_dbg_puts("internal error: this succeeded\n");
        return;
    }

    microkit_dbg_puts("Failed successfully to call CPU_ON\n");
}

void notified(microkit_channel ch)
{
}
