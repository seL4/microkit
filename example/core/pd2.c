#include "core.h"

#define PD1_CHANNEL 1

uintptr_t buffer_vaddr;

void init(void) {
    microkit_dbg_puts("[PD 2]: Starting!\n");
}

void notified(microkit_channel ch) {
    if (ch != PD1_CHANNEL) {
        microkit_dbg_puts("Received unexpected notification\n");
        return;
    }

    switch (((char *) buffer_vaddr)[0]) {
    case 'm':
        migrate_cpu();
        break;
    case 'x':
        microkit_dbg_puts("[PD 2]: Received notification from PD 1 and is turning off core #");
        print_num(current_cpu);
        microkit_dbg_puts("\n");
        
        turn_off_cpu();
        break;
    }
}