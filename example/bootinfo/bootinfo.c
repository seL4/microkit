/*
 * Copyright 2026, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

#if defined(CONFIG_ARCH_X86_64)
typedef struct {
    seL4_BootInfoHeader header;
    uint32_t value;
} bootinfo_tsc_freq_t;
bootinfo_tsc_freq_t *bootinfo_tsc_freq;
#else
#error "No example cases for this architecture"
#endif

void init(void)
{
    microkit_dbg_puts("====BootInfo====\n");
#if defined(CONFIG_ARCH_X86_64)
    if (bootinfo_tsc_freq->header.len) {
        microkit_dbg_puts("TSC Frequency: ");
        microkit_dbg_put32(bootinfo_tsc_freq->value);
        microkit_dbg_puts("MHz\n");
    } else {
        microkit_dbg_puts("TSC Frequency is not found or prefilled properly.\n");
    }
#else
#error "No example cases for this architecture"
#endif
}

void notified(microkit_channel ch)
{
}
