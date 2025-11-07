/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include "loader.h"

#include "arch.h"
#include "cutil.h"
#include "uart.h"

_Static_assert(sizeof(uintptr_t) == 8 || sizeof(uintptr_t) == 4, "Expect uintptr_t to be 32-bit or 64-bit");

#if UINTPTR_MAX == 0xffffffffUL
#define WORD_SIZE 32
#else
#define WORD_SIZE 64
#endif

#if WORD_SIZE == 32
#define MAGIC 0x5e14dead
#else
#define MAGIC 0x5e14dead14de5ead
#endif

#define STACK_SIZE 4096

typedef void (*sel4_entry)(
    uintptr_t ui_p_reg_start,
    uintptr_t ui_p_reg_end,
    intptr_t pv_offset,
    uintptr_t v_entry,
    uintptr_t dtb_addr_p,
    uintptr_t dtb_size,
    uintptr_t extra_device_addr_p,
    uintptr_t extra_device_size
);

extern char _text;
extern char _bss_end;
const struct loader_data *loader_data = (void *) &_bss_end;

char _stack[STACK_SIZE] ALIGN(16);

/*
 * Print out the loader data structure.
 *
 * This doesn't *do anything*. It helps when
 * debugging to verify that the data structures are
 * being interpreted correctly by the loader.
 */
static void print_flags(void)
{
    if (loader_data->flags & FLAG_SEL4_HYP) {
        puts("             seL4 configured as hypervisor\n");
    }
}

static void print_loader_data(void)
{
    puts("LDR|INFO: Flags:                ");
    puthex64(loader_data->flags);
    puts("\n");
    print_flags();
    puts("LDR|INFO: Kernel:      entry:   ");
    puthex64(loader_data->kernel_entry);
    puts("\n");

    puts("LDR|INFO: Root server: physmem: ");
    puthex64(loader_data->ui_p_reg_start);
    puts(" -- ");
    puthex64(loader_data->ui_p_reg_end);
    puts("\nLDR|INFO:              virtmem: ");
    puthex64(loader_data->ui_p_reg_start - loader_data->pv_offset);
    puts(" -- ");
    puthex64(loader_data->ui_p_reg_end - loader_data->pv_offset);
    puts("\nLDR|INFO:              entry  : ");
    puthex64(loader_data->v_entry);
    puts("\n");

    for (uint32_t i = 0; i < loader_data->num_regions; i++) {
        const struct region *r = &loader_data->regions[i];
        puts("LDR|INFO: region: ");
        puthex32(i);
        puts("   addr: ");
        puthex64(r->load_addr);
        puts("   size: ");
        puthex64(r->size);
        puts("   offset: ");
        puthex64(r->offset);
        puts("   type: ");
        puthex64(r->type);
        puts("\n");
    }
}

static void copy_data(void)
{
    const void *base = &loader_data->regions[loader_data->num_regions];
    for (uint32_t i = 0; i < loader_data->num_regions; i++) {
        const struct region *r = &loader_data->regions[i];
        puts("LDR|INFO: copying region ");
        puthex32(i);
        puts("\n");
        memcpy((void *)(uintptr_t)r->load_addr, base + r->offset, r->size);
    }
}

static void start_kernel(void)
{
    ((sel4_entry)(loader_data->kernel_entry))(
        loader_data->ui_p_reg_start,
        loader_data->ui_p_reg_end,
        loader_data->pv_offset,
        loader_data->v_entry,
        0,
        0,
        0,
        0
    );
}

void relocation_failed(void)
{
    puts("LDR|ERROR: relocation failed, loader destination would overlap current loader location\n");
    while (1);
}

void relocation_log(uint64_t reloc_addr, uint64_t curr_addr)
{
    /* This function is called from assembly before main so we call uart_init here as well. */
    uart_init();
    puts("LDR|INFO: relocating from ");
    puthex64(curr_addr);
    puts(" to ");
    puthex64(reloc_addr);
    puts("\n");
}

int main(void)
{
    int r;

    uart_init();
    /* After any UART initialisation is complete, setup an arch-specific exception
     * handler in case we fault somewhere in the loader. */
    arch_set_exception_handler();

    arch_init();

    puts("LDR|INFO: altloader for seL4 starting\n");
    /* Check that the loader magic number is set correctly */
    if (loader_data->magic != MAGIC) {
        puts("LDR|ERROR: mismatch on loader data structure magic number\n");
        goto fail;
    }

    print_loader_data();

    /* past here we have trashed u-boot so any errors should go to the
     * fail label; it's not possible to return to U-boot
     */
    copy_data();

    puts("LDR|INFO: enabling MMU\n");
    r = arch_mmu_enable();
    if (r != 0) {
        goto fail;
    }

    puts("LDR|INFO: jumping to kernel\n");
    start_kernel();

    puts("LDR|ERROR: seL4 Loader: Error - KERNEL RETURNED\n");

fail:
    /* Note: can't usefully return to U-Boot once we are here. */
    /* IMPROVEMENT: use SMC SVC call to try and power-off / reboot system.
     * or at least go to a WFI loop
     */
    for (;;) {
    }
}
