#include "core.h"

/* Definitions for the PL011 UART. Adjust the base address as required. */
#define UART_IRQ_CH 1
#define PD2_CHANNEL 2

void init(void) {
    microkit_dbg_puts("[PD 1]: Hello World!\n");
    uart_init();
    seL4_IRQHandler_SetCore(BASE_IRQ_CAP + UART_IRQ_CH, 2);
}

void notified(microkit_channel ch) {
    if (ch == UART_IRQ_CH) {
        int c = uart_get_char();
        if (c == 'm') {
            migrate_cpu();
        } else if (c == 'd') {
            microkit_dbg_puts("[PD 1]: Received interrupt and is turning off core #");
            print_num(current_cpu);
            microkit_dbg_puts("\n");

            turn_off_cpu();
        } else if (c == 'p') {
            print_psci_version();
        } else if (c == 'n') {
            microkit_dbg_puts("Notifying PD 2\n");
            microkit_notify(PD2_CHANNEL);
        } else if (c == 'r') {
            turn_on_cpu((seL4_Word) init);
        } else if (c == 's') {
            seL4_DebugDumpScheduler();
        }

        uart_handle_irq();
        microkit_irq_ack(ch);
    }
}