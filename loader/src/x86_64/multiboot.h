/*
 * Copyright 2023, Neutrality.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

/*
 * Multiboot 1
 */

#define MULTIBOOT1_HEADER_MAGIC                 0x1BADB002

#define MULTIBOOT1_HEADER_FLAG_PAGE_ALIGN       (1 << 0)
#define MULTIBOOT1_HEADER_FLAG_REQ_MEMORY       (1 << 1)
#define MULTIBOOT1_HEADER_FLAG_REQ_VIDEO        (1 << 2)

#define MULTIBOOT1_HEADER_GFX_MODE_GFX          0
#define MULTIBOOT1_HEADER_GFX_MODE_TEXT         1

#define MULTIBOOT1_BOOT_MAGIC                   0x2BADB002

/*
 * Multiboot 2
 */

#define MULTIBOOT2_HEADER_MAGIC                 0xE85250D6

#define MULTIBOOT2_BOOT_MAGIC                   0x36d76289

#define MULTIBOOT2_INFO_TAG_END                 0
#define MULTIBOOT2_INFO_TAG_COMMAND_LINE        1
#define MULTIBOOT2_INFO_TAG_BOOTLOADER_NAME     2
#define MULTIBOOT2_INFO_TAG_MODULE              3
#define MULTIBOOT2_INFO_TAG_MEMORY              4
#define MULTIBOOT2_INFO_TAG_BIOS_BOOT_DEVICE    5
#define MULTIBOOT2_INFO_TAG_MEMORY_MAP          6
#define MULTIBOOT2_INFO_TAG_VBE_INFO            7
#define MULTIBOOT2_INFO_TAG_FRAMEBUFFER_INFO    8
#define MULTIBOOT2_INFO_TAG_ELF_SYMBOLS         9
#define MULTIBOOT2_INFO_TAG_APM_TABLE           10
#define MULTIBOOT2_INFO_TAG_EFI32_SYSTEM_TABLE  11
#define MULTIBOOT2_INFO_TAG_EFI64_SYSTEM_TABLE  12
#define MULTIBOOT2_INFO_TAG_SMBIOS_TABLES       13
#define MULTIBOOT2_INFO_TAG_ACPI_OLD_RSDP       14
#define MULTIBOOT2_INFO_TAG_ACPI_NEW_RSDP       15
#define MULTIBOOT2_INFO_TAG_NETWORK_INFO        16
#define MULTIBOOT2_INFO_TAG_EFI_MEMORY_MAP      17
#define MULTIBOOT2_INFO_TAG_EFI_BOOTSVC_RUNNING 18
#define MULTIBOOT2_INFO_TAG_EFI32_IMAGE_HANDLE  19
#define MULTIBOOT2_INFO_TAG_EFI64_IMAGE_HANDLE  20
#define MULTIBOOT2_INFO_TAG_LOAD_BASE_PADDR     21
#define MULTIBOOT2_INFO_TAG_DEVICE_MEMORY       42      /* Custom extension. */

#ifndef __ASM__HEADER__

struct multiboot2_tag {
    uint32_t type;
    uint32_t size;
} __attribute__((packed));

struct multiboot2_tag_module {
    struct multiboot2_tag head;
    uint32_t mod_start;
    uint32_t mod_end;
    char cmdline[0];
} __attribute__((packed));

struct multiboot2_tag_device_memory {
    struct multiboot2_tag head;
    uint64_t dmem_addr;
    uint64_t dmem_size;
} __attribute__((packed));

#endif
