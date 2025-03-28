#pragma once

#include <microkit.h>
#include "uart.h"

#define PSCI_CPU_OFF 0x84000002
#define PSCI_CPU_ON 0xC4000003

#define PSCI_VERSION_FID 0x84000000

int current_cpu = 0;

static void migrate_cpu() {
    int new_cpu = ++current_cpu % 4;
    microkit_dbg_puts("migrating to CPU ");
    print_num(new_cpu);
    microkit_dbg_puts("\n");

    seL4_SchedControl_ConfigureFlags(
        BASE_SCHED_CONTROL_CAP + new_cpu,
        BASE_SCHED_CONTEXT_CAP,
        microkit_pd_period,
        microkit_pd_budget,
        microkit_pd_extra_refills,
        microkit_pd_badge,
        microkit_pd_flags
    );
}

static void turn_off_cpu() {
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_CPU_OFF;

    microkit_arm_smc_call(&args, &response);
}

static void turn_on_cpu(seL4_Word entry) {
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_CPU_ON;
    args.x1 = 3;                    /* target CPU id (for example, core 3) */
    args.x2 = entry;                /* entry point */
    args.x3 = 0;                    /* context id (unused here) */

    microkit_arm_smc_call(&args, &response);

    microkit_dbg_puts("response: ");
    print_num(response.x0);
    microkit_dbg_puts("\n");
}

static void print_psci_version() {
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext resp = {0};

    args.x0 = PSCI_VERSION_FID;
    microkit_arm_smc_call(&args, &resp);

    microkit_dbg_puts("PSCI version: ");
    print_num(((uint32_t) resp.x0 >> 16) & 0xFFFF);
    microkit_dbg_puts(".");
    print_num((uint32_t) resp.x0 & 0xFFFF);
    microkit_dbg_puts("\n");
}