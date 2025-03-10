/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>

#define __thread
#include <sel4/sel4.h>

#include "util.h"
#include "debug.h"

void dump_bootinfo(seL4_BootInfo *bi)
{
    unsigned i;

    puts("Bootinfo: ");
    puthex64((uintptr_t)bi);
    puts("\n");

    puts("extraLen                = ");
    puthex64(bi->extraLen);
    puts("\n");

    puts("nodeID                  = ");
    puthex64(bi->nodeID);
    puts("\n");

    puts("numNodes                = ");
    puthex64(bi->numNodes);
    puts("\n");

    puts("numIOPTLevels           = ");
    puthex64(bi->numIOPTLevels);
    puts("\n");

    puts("ipcBuffer*              = ");
    puthex64((uintptr_t)bi->ipcBuffer);
    puts("\n");

    puts("initThreadCNodeSizeBits = ");
    puthex64(bi->initThreadCNodeSizeBits);
    puts("\n");

    puts("initThreadDomain        = ");
    puthex64(bi->initThreadDomain);
    puts("\n");

    puts("userImagePaging         = ");
    puthex64(bi->userImagePaging.start);
    puts("..");
    puthex64(bi->userImagePaging.end - 1);
    puts("\n");

    puts("schedcontrol            = ");
    puthex64(bi->schedcontrol.start);
    puts("..");
    puthex64(bi->schedcontrol.end - 1);
    puts("\n");

    puts("userImageFrames         = ");
    puthex64(bi->userImageFrames.start);
    puts("..");
    puthex64(bi->userImageFrames.end - 1);
    puts("\n");

    puts("untyped                 = ");
    puthex64(bi->untyped.start);
    puts("..");
    puthex64(bi->untyped.end - 1);
    puts("\n");

    puts("empty                   = ");
    puthex64(bi->empty.start);
    puts("..");
    puthex64(bi->empty.end - 1);
    puts("\n");

    puts("sharedFrames            = ");
    puthex64(bi->sharedFrames.start);
    puts("..");
    puthex64(bi->sharedFrames.end - 1);
    puts("\n");

    puts("ioSpaceCaps             = ");
    puthex64(bi->ioSpaceCaps.start);
    puts("..");
    puthex64(bi->ioSpaceCaps.end - 1);
    puts("\n");

    puts("extraBIPages            = ");
    puthex64(bi->extraBIPages.start);
    puts("..");
    puthex64(bi->extraBIPages.end - 1);
    puts("\n");

#if 1
    for (i = 0; i < bi->untyped.end - bi->untyped.start; i++) {
        puts("untypedList[");
        puthex32(i);
        puts("]        = slot: ");
        puthex32(bi->untyped.start + i);
        puts(", paddr: ");
        puthex64(bi->untypedList[i].paddr);
        puts(" - ");
        puthex64(bi->untypedList[i].paddr + (1UL << bi->untypedList[i].sizeBits));
        puts(" (");
        puts(bi->untypedList[i].isDevice ? "device" : "normal");
        puts(") bits: ");
        puthex32(bi->untypedList[i].sizeBits);
        puts("\n");
    }
#endif
    /* The extended printing over the individual untypes is good if you care
       about the individual objects, but annoying if you want to focus on memory
       regions. This coalesces thing before printing to summarize the regions.
       This works best when the input is sorted! In practise untyped are sorted
       by device/normal and then address, so coalescing works well, but not perfectly.
       Good enough for debug.

       Note: the 'gaps' we see are where the kernel is using the memory. For device
       memory, this is the memory regions of the GIC. For regular memory that is
       memory used for kernel and rootserver.
    */
#if 1
    puts("\nBoot Info Untyped Memory Ranges\n");
    seL4_Word start = bi->untypedList[0].paddr;
    seL4_Word end = start + (1ULL << bi->untypedList[0].sizeBits);
    seL4_Word is_device = bi->untypedList[0].isDevice;
    for (i = 1; i < bi->untyped.end - bi->untyped.start; i++) {
        if (bi->untypedList[i].paddr != end || bi->untypedList[i].isDevice != is_device) {
            puts("                                     paddr: ");
            puthex64(start);
            puts(" - ");
            puthex64(end);
            puts(" (");
            puts(is_device ? "device" : "normal");
            puts(")\n");
            start = bi->untypedList[i].paddr;
            end = start + (1ULL << bi->untypedList[i].sizeBits);
            is_device = bi->untypedList[i].isDevice;
        } else {
            end += (1ULL << bi->untypedList[i].sizeBits);
        }
    }
    puts("                                     paddr: ");
    puthex64(start);
    puts(" - ");
    puthex64(end);
    puts(" (");
    puts(is_device ? "device" : "normal");
    puts(")\n");
#endif
}
