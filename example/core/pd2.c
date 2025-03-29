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
    case 's':
        microkit_dbg_puts("\n=== THE FOLLOWING DUMP IS FOR PROTECTION DOMAINS RUNNING ON [PD 2]'s CORE ===\n");
        seL4_DebugDumpScheduler();
        break;
    case 'n':
        core_migrate(1);
        break;
    case 'x':
        microkit_dbg_puts("[PD 2]: Turning off core #");
        print_num(current_cpu);
        microkit_dbg_puts("\n");
        
        core_off();
        break;
    }
}