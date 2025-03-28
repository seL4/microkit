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
    case 'd':
        microkit_dbg_puts("[PD 1]: Received interrupt and is turning off core #");
        print_num(current_cpu);
        microkit_dbg_puts("\n");

        turn_off_cpu();
        break;
    case 'p':
        print_psci_version();
        break;
    case 'x':
        microkit_notify(PD2_CHANNEL);
        break;
    case 's':
        seL4_DebugDumpScheduler();
        break;
    }

    microkit_irq_ack(ch);
}