/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include "../arch.h"
#include "../uart.h"
#include "el.h"

#include <kernel/gen_config.h>

#if defined(CONFIG_PLAT_ZYNQMP_ZCU102) || defined(CONFIG_PLAT_ZYNQMP_ULTRA96V2)
#define GICD_BASE 0x00F9010000UL
#define GICC_BASE 0x00F9020000UL
#elif defined(CONFIG_PLAT_QEMU_ARM_VIRT)
#define GICD_BASE 0x8000000UL
#define GICC_BASE 0x8010000UL
#endif

#if defined(CONFIG_PLAT_ZYNQMP_ZCU102) || defined(CONFIG_PLAT_ZYNQMP_ULTRA96V2) || defined(CONFIG_PLAT_QEMU_ARM_VIRT)
static void configure_gicv2(void)
{
    /* The ZCU102 start in EL3, and then we drop to EL1(NS).
     *
     * The GICv2 supports security extensions (as does the CPU).
     *
     * The GIC sets any interrupt as either Group 0 or Group 1.
     * A Group 0 interrupt can only be configured in secure mode,
     * while Group 1 interrupts can be configured from non-secure mode.
     *
     * As seL4 runs in non-secure mode, and we want seL4 to have
     * the ability to configure interrupts, at this point we need
     * to put all interrupts into Group 1.
     *
     * GICD_IGROUPn starts at offset 0x80.
     *
     * 0xF901_0000.
     *
     * Future work: On multicore systems the distributor setup
     * only needs to be called once, while the GICC registers
     * should be set for each CPU.
     */
    puts("LDR|INFO: Setting all interrupts to Group 1\n");
    uint32_t gicd_typer = *((volatile uint32_t *)(GICD_BASE + 0x4));
    uint32_t it_lines_number = gicd_typer & 0x1f;
    puts("LDR|INFO: GICv2 ITLinesNumber: ");
    puthex32(it_lines_number);
    puts("\n");

    for (uint32_t i = 0; i <= it_lines_number; i++) {
        *((volatile uint32_t *)(GICD_BASE + 0x80 + (i * 4))) = 0xFFFFFFFF;
    }

    /* For any interrupts to go through the interrupt priority mask
     * must be set appropriately. Only interrupts with priorities less
     * than this mask will interrupt the CPU.
     *
     * seL4 (effectively) sets interrupts to priority 0x80, so it is
     * important to make sure this is greater than 0x80.
     */
    *((volatile uint32_t *)(GICC_BASE + 0x4)) = 0xf0;
}
#endif

void el1_mmu_disable(void);
void el2_mmu_disable(void);

void arch_init(void)
{
#if defined(CONFIG_PLAT_ZYNQMP_ZCU102) || defined(CONFIG_PLAT_ZYNQMP_ULTRA96V2) || defined(CONFIG_PLAT_QEMU_ARM_VIRT)
    configure_gicv2();
#endif

    /* Disable the MMU, as U-Boot will start in virtual memory on some platforms
     * (https://docs.u-boot.org/en/latest/arch/arm64.html), which means that
     * certain physical memory addresses contain page table information which
     * the loader doesn't know about and would need to be careful not to
     * overwrite.
     *
     * This also means that we would need to worry about caching.
     * TODO: should we do that instead?
     * note the issues where it forces us to flush any shared addresses all the
     * way to cache as we might have mixed non-cached/cached access.
     */
    puts("LDR|INFO: disabling MMU (if it was enabled)\n");
    enum el el = current_el();

    if (el == EL1) {
        el1_mmu_disable();
    } else if (el == EL2) {
        el2_mmu_disable();
    } else {
        puts("LDR|ERROR: unknown EL level for MMU disable\n");
    }
}
