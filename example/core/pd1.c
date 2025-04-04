#include "core.h"

#define PD2_CHANNEL 2

uintptr_t buffer_vaddr;

void init(void) {
    microkit_dbg_puts("[PD 1]: Starting!\n");
    uart_init();
}

void notified(microkit_channel ch) {
    if (ch != UART_IRQ_CH) {
        microkit_dbg_puts("Received unexpected notification\n");
        return;
    }

    ((char *) buffer_vaddr)[0] = uart_get_char();
    uart_handle_irq();

    switch (((char *) buffer_vaddr)[0]) {
    case 'h':
        microkit_dbg_puts(
            "\n=== LIST OF COMMANDS ===\n"
            "h: help\n"
            "p: print psci version\n"
            "i: view the status of core #0\n"
            "d: core dump\n"
            "m: migrate pd1\n"
            "n: migrate pd2\n"
            "x: turn off pd2's core\n"
            "s: put pd2's core in standby\n"
            "y: turn on pd2's core\n"
        );    
        break;
    case 'p':
        print_psci_version();
        break;
    case 'd':
        microkit_dbg_puts("=== THE FOLLOWING DUMP IS FOR PROTECTION DOMAINS RUNNING ON PD1's CORE ===\n");
        seL4_DebugDumpScheduler();
        microkit_notify(PD2_CHANNEL);
        break;
    case 's':
        microkit_notify(PD2_CHANNEL);
        break;
    case 'm':
        core_migrate(0);
        seL4_IRQHandler_SetCore(BASE_IRQ_CAP + UART_IRQ_CH, 1);
        break;
    case 'n':
        microkit_notify(PD2_CHANNEL);
        break;
    case 'x':
        microkit_notify(PD2_CHANNEL);
        break;
    case 'y':
        microkit_dbg_puts("[PD 1]: Turning on core #0\n");
        core_on(0);
        break;
    case 'i':
        microkit_dbg_puts("[PD 1]: Viewing status of core #0\n");
        core_status(0);
        break;
    }

    microkit_irq_ack(ch);
}