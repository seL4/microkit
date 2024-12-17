/*
 * Copyright 2023, Neutrality.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stdint.h>
#include "utils.h"
#include "multiboot.h"

/*
 * These global variables are overwritten by the microkit tool when building the
 * image.
 */
uint32_t kernel_entry;
uint32_t monitor_addr;
uint32_t monitor_size;
uint64_t extra_device_addr_p;
uint64_t extra_device_size;

/* Name the initial task. This adds nothing but flare to the boot logs. */
static char *monitor_cmdline = "microkit";

/* Hardcode the serial port address.
 * @mat: one day this should be configurable. */
static const uint16_t serial_port = 0x3f8;

/* Round a number up to the next 64-bit boundary. */
static uint32_t roundup64(uint32_t n)
{
    if (n & 7)
        n = (n & ~7) + 8;
    return n;
}

/* Serial code taken from seL4/src/plat/pc99/machine/io.c */
static void serial_init(void)
{
    while (!(in8(serial_port + 5) & 0x60)) /* wait until not busy */
        ;

    out8(serial_port + 1, 0x00); /* disable generating interrupts */
    out8(serial_port + 3, 0x80); /* line control register: command: set divisor */
    out8(serial_port,     0x01); /* set low byte of divisor to 0x01 = 115200 baud */
    out8(serial_port + 1, 0x00); /* set high byte of divisor to 0x00 */
    out8(serial_port + 3, 0x03); /* line control register: set 8 bit, no parity, 1 stop bit */
    out8(serial_port + 4, 0x0b); /* modem control register: set DTR/RTS/OUT2 */

    in8(serial_port);     /* clear receiver serial_port */
    in8(serial_port + 5); /* clear line status serial_port */
    in8(serial_port + 6); /* clear modem status serial_port */
}

static inline void putc(uint8_t ch)
{
    while ((in8(serial_port + 5) & 0x20) == 0)
        ;
    out8(serial_port, ch);
}

static inline void puts(const char *s)
{
    while (*s)
        putc(*s++);
}

static int loader_multiboot2(uint32_t multiboot_info_ptr)
{
    uint32_t *total_size = (uint32_t *) multiboot_info_ptr;
    uint32_t last_tag_offset = 0;

    /* Walk the list of multiboot info tags. */
    for (uint32_t i = 2 * sizeof (uint32_t); i < *total_size; ) {
        struct multiboot2_tag *tag = (void *) (multiboot_info_ptr + i);

        /* Fail if we were given any multiboot module. */
        if (tag->type == MULTIBOOT2_INFO_TAG_MODULE) {
            puts("LDR|ERROR: multiboot modules not supported\r\n");
            return -1;
        }

        /* Break on the closing tag. */
        if (tag->type == MULTIBOOT2_INFO_TAG_END && tag->size == 8) {
            last_tag_offset = i;
            break;
        }

        /* Skip this tag and round up to the next 64-bit boundary. */
        i = roundup64(i + tag->size);
    }

    /* That shouldn't happen but who knows. */
    if (!last_tag_offset) {
        puts("LDR|ERROR: invalid boot information tag list\r\n");
        return -1;
    }

    /*
     * From here onwards we are carelessly extending the list of multiboot2
     * tags without checking that we do not overwrite anything important.
     * So far there seem to be quite a lot of space between this tag list
     * and the next memory region in use so that's good enough for a
     * proof-of-concept implementation, but one this this should really be
     * cleaned up.
     */

    /* Add a module tag for the monitor inittask ELF file. */
    struct multiboot2_tag_module *module = (void *) (multiboot_info_ptr + last_tag_offset);
    module->head.type = MULTIBOOT2_INFO_TAG_MODULE;
    module->head.size = sizeof (*module) + strlen(monitor_cmdline) + 1;
    module->mod_start = monitor_addr;
    module->mod_end   = monitor_addr + monitor_size;
    memcpy(&module->cmdline[0], monitor_cmdline, strlen(monitor_cmdline) + 1);

    /* Account for the new tag. */
    *total_size += roundup64(module->head.size);
    last_tag_offset += roundup64(module->head.size);

    /* Add a custom tag to register device memory: memory regions that will
     * be marked as device untyped by the kernel. This is an unofficial
     * addition to the multiboot2 specs. */
    struct multiboot2_tag_device_memory *devmem = (void *) (multiboot_info_ptr + last_tag_offset);
    devmem->head.type = MULTIBOOT2_INFO_TAG_DEVICE_MEMORY;
    devmem->head.size = sizeof (*devmem);
    devmem->dmem_addr = extra_device_addr_p;
    devmem->dmem_size = extra_device_size;

    /* Account for the new tag. */
    *total_size += roundup64(devmem->head.size);
    last_tag_offset += roundup64(devmem->head.size);

    /* Add a new end tag to close the list. Note that we do not need to
     * account for this end tag since we have overwritten the previous one
     * which was already accounted for. */
    struct multiboot2_tag *end = (void *) (multiboot_info_ptr + last_tag_offset);
    end->type = MULTIBOOT2_INFO_TAG_END;
    end->size = sizeof (*end);

    puts("LDR|INFO: loading complete, have a safe journey\r\n");
    return 0;
}

int loader(uint32_t multiboot_magic, uint32_t multiboot_info_ptr)
{
    serial_init();

    switch (multiboot_magic) {
    case MULTIBOOT1_BOOT_MAGIC:
        puts("LDR|INFO: booted as Multiboot v1\r\n");
        puts("LDR|ERROR: multiboot v1 not supported\r\n");
        return -1;

    case MULTIBOOT2_BOOT_MAGIC:
        puts("LDR|INFO: booted as Multiboot v2\r\n");
        return loader_multiboot2(multiboot_info_ptr);

    default:
        puts("LDR|ERROR: invalid multiboot magic\r\n");
        return -1;
    }
}
