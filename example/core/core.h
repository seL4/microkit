#pragma once

#include <microkit.h>
#include "uart.h"

#define PSCI_VERSION_FID 0x84000000
#define PSCI_CPU_OFF 0x84000002

#ifdef ARCH_aarch64
#define PSCI_CPU_ON 0xC4000003
#define PSCI_AFFINITY_INFO 0xC4000004
#else
#define PSCI_CPU_ON 0x84000003
#define PSCI_AFFINITY_INFO 0x84000004
#endif

/* Possible Error Codes */
#define PSCI_SUCCESS                0
#define PSCI_E_INVALID_PARAMETERS   ((unsigned long) -2)
#define PSCI_E_DENIED               ((unsigned long) -3)
#define PSCI_E_ALREADY_ON           ((unsigned long) -4)
#define PSCI_E_ON_PENDING           ((unsigned long) -5)
#define PSCI_E_INTERNAL_FAILURE     ((unsigned long) -6)
#define PSCI_E_DISABLED             ((unsigned long) -8)
#define PSCI_E_INVALID_ADDRESS      ((unsigned long) -9)

int current_cpu = 0;

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

static void core_status(int core) {
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_AFFINITY_INFO;
    args.x1 = core;                     /* target CPU id (for example, core 3) */
    args.x2 = 0;                        /* lowest affinity level */

    microkit_arm_smc_call(&args, &response);

    switch (response.x0) {
    case 0:
        microkit_dbg_puts("The CPU core is ON.\n");
        break;
    case 1:
        microkit_dbg_puts("The CPU core is OFF.\n");
        break;
    case PSCI_E_ON_PENDING:
        microkit_dbg_puts("A call to turn a cpu on was already made and is being completed.\n");
        break;
    case PSCI_E_DISABLED:
        microkit_dbg_puts("The specific core you are trying to view the status off is disabled.\n");
        break;
    case PSCI_E_INVALID_PARAMETERS:
        microkit_dbg_puts("Your request to view the status of a cpu core had invalid parameters.\n");
        break;
    default:
        microkit_dbg_puts("Encountered an unexpected case.\n");
        break;
    }
}

static void core_off() {
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_CPU_OFF;

    microkit_arm_smc_call(&args, &response);

    switch (response.x0) {
    case PSCI_SUCCESS:
        microkit_dbg_puts("Successfully turned on the CPU core.\n");
        break;
    case PSCI_E_DENIED:
        microkit_dbg_puts("Your request to turn on the cpu core was denied due to firmware enforced policy.\n");
        break;
    default:
        microkit_dbg_puts("Encountered an unexpected case.\n");
        break;
    }
}

static void awaken_entry() {
    microkit_dbg_puts("A CPU core has re-awakened!\n");
}

static void core_on(uint8_t core) {
    seL4_ARM_SMCContext args = {0};
    seL4_ARM_SMCContext response = {0};
    args.x0 = PSCI_CPU_ON;
    args.x1 = core;                         /* target CPU id (for example, core 3) */
    args.x2 = (seL4_Word) awaken_entry;     /* entry point */
    args.x3 = 0;                            /* context id (unused here) */

    microkit_arm_smc_call(&args, &response);

    switch (response.x0) {
    case PSCI_SUCCESS:
        microkit_dbg_puts("Successfully turned on the CPU core.\n");
        break;
    case PSCI_E_INVALID_PARAMETERS:
        microkit_dbg_puts("Your request to turn on the cpu core had invalid parameters.\n");
        break;
    case PSCI_E_DENIED:
        microkit_dbg_puts("Your request to turn on the cpu core was denied due to firmware enforced policy.\n");
        break;
    case PSCI_E_ALREADY_ON:
        microkit_dbg_puts("The core you are trying to turn on, is already on.\n");
        break;
    case PSCI_E_ON_PENDING:
        microkit_dbg_puts("A call to turn a cpu on was already made and is being completed.\n");
        break;
    case PSCI_E_INTERNAL_FAILURE:
        microkit_dbg_puts("The specific core cannot be powered up due to physical reasons.\n");
        break;
    case PSCI_E_INVALID_ADDRESS:
        microkit_dbg_puts("The provided entry point address for the core is invalid.\n");
        break;
    default:
        microkit_dbg_puts("Encountered an unexpected case.\n");
        break;
    }
}

static void core_migrate(int pd_id) {
    int new_cpu = ++current_cpu % 4;
    microkit_dbg_puts("Migrating PD");
    print_num(pd_id + 1);
    microkit_dbg_puts(" to CPU #");
    print_num(new_cpu);
    microkit_dbg_puts("\n");

    seL4_SchedControl_ConfigureFlags(
        BASE_SCHED_CONTROL_CAP + new_cpu,
        BASE_SCHED_CONTEXT_CAP + pd_id,
        microkit_pd_period,
        microkit_pd_budget,
        microkit_pd_extra_refills,
        microkit_pd_badge,
        microkit_pd_flags
    );
}