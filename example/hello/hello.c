/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>
//#include <types.h>
#include <sel4/bootinfo_types.h>

uintptr_t remaining_untypeds_vaddr;
typedef struct {
    seL4_CNode untyped_cnode_cap;
    seL4_SlotRegion untypeds;
    seL4_UntypedDesc untypedList[CONFIG_MAX_NUM_BOOTINFO_UNTYPED_CAPS];
} capDLBootInfo_t;

capDLBootInfo_t *capDLBootInfo;

void print_64(seL4_Word w) {
    microkit_dbg_put32((seL4_Uint32) (w >> 32));
    microkit_dbg_put32((seL4_Uint32) w);
}
void init(void)
{
    capDLBootInfo = (capDLBootInfo_t*) remaining_untypeds_vaddr;
    microkit_dbg_puts("hello, world\nuntyped_cnode_cap: ");
    microkit_dbg_put32((seL4_Uint32) (capDLBootInfo->untyped_cnode_cap >> 32));
    microkit_dbg_putc(32);
    microkit_dbg_put32((seL4_Uint32) capDLBootInfo->untyped_cnode_cap);
    microkit_dbg_puts("\nend\n");

    microkit_dbg_puts("idx  paddr    sizeBits    isDevice\n");
    for (uint32_t i = capDLBootInfo->untypeds.start; i < capDLBootInfo->untypeds.end; i++) {
        microkit_dbg_put32(i);
        microkit_dbg_puts("  ");
        print_64(capDLBootInfo->untypedList[i].paddr);
        microkit_dbg_puts("  ");
        print_64(capDLBootInfo->untypedList[i].sizeBits);
        microkit_dbg_puts("  ");
        if(capDLBootInfo->untypedList[i].isDevice) {
            microkit_dbg_puts("true\n");
        } else {
            microkit_dbg_puts("false\n");
        }
    }

    // Try retype untyped idx 39
    microkit_dbg_puts("Creating new untyped from Untyped Idx 39 of size 4 at idx 68");
    seL4_Error err = seL4_Untyped_Retype(capDLBootInfo->untyped_cnode_cap + 39, seL4_UntypedObject, 4, capDLBootInfo->untyped_cnode_cap, 0, 0, 68, 1);

    microkit_dbg_puts("seL4_NoError: ");
    microkit_dbg_put32(seL4_NoError);
    microkit_dbg_puts("\n");
    microkit_dbg_puts("err: ");
    microkit_dbg_put32(err);
    microkit_dbg_puts("\n");
}

void notified(microkit_channel ch)
{
}
