/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <strings.h>

// @ivanv: merge this with loader.c aarch64

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

#define ALIGN(n)  __attribute__((__aligned__(n)))

#define MASK(x) ((1U << x) - 1)

#define STACK_SIZE 4096

#define FLAG_SEL4_HYP (1UL << 0)

struct region {
    uintptr_t load_addr;
    uintptr_t size;
    uintptr_t offset;
    uintptr_t type;
};

struct loader_data {
    uintptr_t magic;
    uintptr_t flags;
    uintptr_t kernel_entry;
    uintptr_t ui_p_reg_start;
    uintptr_t ui_p_reg_end;
    uintptr_t pv_offset;
    uintptr_t v_entry;
    uintptr_t extra_device_addr_p;
    uintptr_t extra_device_size;

    uintptr_t num_regions;
    struct region regions[];
};

typedef void (*sel4_entry)(
    uintptr_t ui_p_reg_start,
    uintptr_t ui_p_reg_end,
    intptr_t pv_offset,
    uintptr_t v_entry,
    uintptr_t dtb_addr_p,
    uintptr_t dtb_size,
    // uintptr_t hart_id,
    // uintptr_t core_id,
    uintptr_t extra_device_addr_p,
    uintptr_t extra_device_size
);

char _stack[STACK_SIZE] ALIGN(16);

/* Paging structures for kernel mapping */
uint64_t boot_lvl1_pt[1 << 9] ALIGN(1 << 12);
uint64_t boot_lvl2_pt[1 << 9] ALIGN(1 << 12);
/* Paging structures for identity mapping */
uint64_t boot_lvl2_pt_elf[1 << 9] ALIGN(1 << 12);

extern char _text;
extern char _text_end;
extern char _bss_end;
const struct loader_data *loader_data = (void *)&_bss_end;

static void
memcpy(void *dst, const void *src, size_t sz)
{
    char *dst_ = dst;
    const char *src_ = src;
    while (sz-- > 0) {
        *dst_++ = *src_++;
    }
}

#define SBI_CONSOLE_PUTCHAR 1

#define SBI_CALL(which, arg0, arg1, arg2) ({            \
    register uintptr_t a0 asm ("a0") = (uintptr_t)(arg0);   \
    register uintptr_t a1 asm ("a1") = (uintptr_t)(arg1);   \
    register uintptr_t a2 asm ("a2") = (uintptr_t)(arg2);   \
    register uintptr_t a7 asm ("a7") = (uintptr_t)(which);  \
    asm volatile ("ecall"                   \
              : "+r" (a0)               \
              : "r" (a1), "r" (a2), "r" (a7)        \
              : "memory");              \
    a0;                         \
})

#define SBI_CALL_1(which, arg0) SBI_CALL(which, arg0, 0, 0)

static void
putc(uint8_t ch)
{
    SBI_CALL_1(SBI_CONSOLE_PUTCHAR, ch);
}

static void
puts(const char *s)
{
    while (*s) {
        putc(*s);
        s++;
    }
}

static char
hexchar(unsigned int v)
{
    return v < 10 ? '0' + v : ('a' - 10) + v;
}

static void
puthex32(uint32_t val)
{
    char buffer[8 + 3];
    buffer[0] = '0';
    buffer[1] = 'x';
    buffer[8 + 3 - 1] = 0;
    for (unsigned i = 8 + 1; i > 1; i--) {
        buffer[i] = hexchar(val & 0xf);
        val >>= 4;
    }
    puts(buffer);
}

static void
puthex64(uint64_t val)
{
    char buffer[16 + 3];
    buffer[0] = '0';
    buffer[1] = 'x';
    buffer[16 + 3 - 1] = 0;
    for (unsigned i = 16 + 1; i > 1; i--) {
        buffer[i] = hexchar(val & 0xf);
        val >>= 4;
    }
    puts(buffer);
}

/*
 * Print out the loader data structure.
 *
 * This doesn't *do anything*. It helps when
 * debugging to verify that the data structures are
 * being interpretted correctly by the loader.
 */
static void
print_flags(void)
{
    if (loader_data->flags & FLAG_SEL4_HYP) {
        puts("             seL4 configured as hypervisor\n");
    }
}

static void
print_loader_data(void)
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

static void
copy_data(void)
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

static void
start_kernel(void)
{
}

int
main(void)
{
    puts("LDR|INFO: altloader for seL4 starting\n");
    /* Check that the loader magic number is set correctly */
    if (loader_data->magic != MAGIC) {
        puts("LDR|ERROR: mismatch on loader data structure magic number\n");
        return 1;
    }

    print_loader_data();

    copy_data();

    puts("LDR|INFO: jumping to kernel\n");
    start_kernel();

    puts("LDR|ERROR: seL4 Loader: Error - KERNEL RETURNED\n");
    goto fail;

fail:
}
