#include "core.h"

#define PD2_CHANNEL 2

uintptr_t buffer_vaddr;

void init(void) {
    microkit_dbg_puts("[PD 1]: Starting!\n");
    uart_init();
    seL4_IRQHandler_SetCore(BASE_IRQ_CAP + UART_IRQ_CH, 2);
}

void notified(microkit_channel ch) {
    if (ch != UART_IRQ_CH) {
        microkit_dbg_puts("Received unexpected notification\n");
        return;
    }

    ((char *) buffer_vaddr)[0] = uart_get_char();
    uart_handle_irq();

    switch (((char *) buffer_vaddr)[0]) {
    case 'm':
        microkit_notify(PD2_CHANNEL);
        break;
    case 'p':
        print_psci_version();
        break;
    case 'x':
        microkit_notify(PD2_CHANNEL);
        break;
    case 's':
        microkit_dbg_puts("=== THE FOLLOWING DUMP IS FOR PROTECTION DOMAINS RUNNING ON PD1's CORE ===\n");
        seL4_DebugDumpScheduler();
        microkit_notify(PD2_CHANNEL);
        break;
    }

    microkit_irq_ack(ch);
}