#include "core.h"

#define PD1_CHANNEL 1

void init(void) {
    microkit_dbg_puts("[PD 2]: Hello World!\n");
}

void notified(microkit_channel ch) {
    switch (ch) {
        case PD1_CHANNEL: {
            microkit_dbg_puts("[PD 2]: Received notification from PD 1 and is turning off core #");
            print_num(current_cpu);
            microkit_dbg_puts("\n");
            
            turn_off_cpu();
            break;
        }
    }
}