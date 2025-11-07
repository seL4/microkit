/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include "../uart.h"
#include "../arch.h"
#include "el.h"

#if defined(BOARD_zcu102) || defined(BOARD_ultra96v2)
#define GICD_BASE 0x00F9010000UL
#define GICC_BASE 0x00F9020000UL
#elif defined(BOARD_qemu_virt_aarch64)
#define GICD_BASE 0x8000000UL
#define GICC_BASE 0x8010000UL
#endif

#if defined(BOARD_zcu102) || defined(BOARD_ultra96v2) || defined(BOARD_qemu_virt_aarch64)
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

void arch_init(void)
{
#if defined(BOARD_zcu102) || defined(BOARD_ultra96v2) || defined(BOARD_qemu_virt_aarch64)
    configure_gicv2();
#endif
}
