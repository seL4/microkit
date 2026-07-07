/*
 * Copyright 2026, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include <microkit.h>

#define PCI_CONFIG_ADDRESS 0xcf8u
#define PCI_CONFIG_DATA 0xcfcu
#define PCI_ENABLE 0x80000000u
#define PCI_COMMAND 0x04u
#define PCI_BAR0 0x10u
#define PCI_COMMAND_MEMORY 0x0002u
#define PCI_COMMAND_MASTER 0x0004u

#define EDU_BUS 0u
#define EDU_DEV 4u
#define EDU_FUNC 0u
#define EDU_DEVICE_ID 0xedu
#define EDU_PCI_VENDOR_ID 0x1234u
#define EDU_PCI_DEVICE_ID 0x11e8u

#define EDU_MMIO_PADDR 0xfe000000ull
#define EDU_MMIO_SIZE 0x00100000ull
#define EDU_BAR_SIZE 0x00100000ull
#define EDU_DMA_SRAM 0x40000ull
#define DMA_BUFFER_IOVA 0x00100000ull
#define DMA_TEST_BYTES 64u
#define DMA_RETURN_OFFSET 0x100u
#define DMA_TIMEOUT 10000000u

#define EDU_REG_ID 0x00u
#define EDU_REG_ALIVE 0x04u
#define EDU_REG_DMA_SRC 0x80u
#define EDU_REG_DMA_DST 0x88u
#define EDU_REG_DMA_COUNT 0x90u
#define EDU_REG_DMA_CMD 0x98u

#define EDU_DMA_START 0x1u
#define EDU_DMA_TO_RAM 0x2u

uint64_t edu_mmio_window_vaddr;
uint64_t dma_buffer_vaddr;
uint64_t pci_config_ioport_id;

static void put_hex64(uint64_t value)
{
    char buf[19] = "0x0000000000000000";
    for (size_t i = 0; i < 16; i++) {
        unsigned int nibble = (value >> ((15 - i) * 4)) & 0xf;
        buf[2 + i] = nibble < 10 ? '0' + nibble : 'a' + (nibble - 10);
    }
    microkit_dbg_puts(buf);
}

static uint32_t pci_config_read32(uint8_t bus, uint8_t dev, uint8_t func, uint8_t offset)
{
    uint32_t address =
        PCI_ENABLE | ((uint32_t)bus << 16) | ((uint32_t)dev << 11) | ((uint32_t)func << 8) | (offset & 0xfcu);

    microkit_x86_ioport_write_32(pci_config_ioport_id, PCI_CONFIG_ADDRESS, address);
    return microkit_x86_ioport_read_32(pci_config_ioport_id, PCI_CONFIG_DATA);
}

static uint16_t pci_config_read16(uint8_t bus, uint8_t dev, uint8_t func, uint8_t offset)
{
    uint32_t value = pci_config_read32(bus, dev, func, offset);
    return (value >> ((offset & 2u) * 8)) & 0xffffu;
}

static void pci_config_write16(uint8_t bus, uint8_t dev, uint8_t func, uint8_t offset, uint16_t value)
{
    uint32_t address =
        PCI_ENABLE | ((uint32_t)bus << 16) | ((uint32_t)dev << 11) | ((uint32_t)func << 8) | (offset & 0xfcu);

    microkit_x86_ioport_write_32(pci_config_ioport_id, PCI_CONFIG_ADDRESS, address);
    microkit_x86_ioport_write_16(pci_config_ioport_id, PCI_CONFIG_DATA + (offset & 2u), value);
}

static void pci_config_write32(uint8_t bus, uint8_t dev, uint8_t func, uint8_t offset, uint32_t value)
{
    uint32_t address =
        PCI_ENABLE | ((uint32_t)bus << 16) | ((uint32_t)dev << 11) | ((uint32_t)func << 8) | (offset & 0xfcu);

    microkit_x86_ioport_write_32(pci_config_ioport_id, PCI_CONFIG_ADDRESS, address);
    microkit_x86_ioport_write_32(pci_config_ioport_id, PCI_CONFIG_DATA, value);
}

static volatile uint32_t *edu_reg32(uintptr_t edu_base, uint32_t offset)
{
    return (volatile uint32_t *)(edu_base + offset);
}

static volatile uint64_t *edu_reg64(uintptr_t edu_base, uint32_t offset)
{
    return (volatile uint64_t *)(edu_base + offset);
}

static uint32_t edu_read32(uintptr_t edu_base, uint32_t offset)
{
    return *edu_reg32(edu_base, offset);
}

static void edu_write32(uintptr_t edu_base, uint32_t offset, uint32_t value)
{
    *edu_reg32(edu_base, offset) = value;
}

static void edu_write64(uintptr_t edu_base, uint32_t offset, uint64_t value)
{
    *edu_reg64(edu_base, offset) = value;
}

static void wait_for_dma(uintptr_t edu_base)
{
    while ((edu_read32(edu_base, EDU_REG_DMA_CMD) & EDU_DMA_START))
        ;
}

static void fill_dma_buffer(void)
{
    volatile uint32_t *buf = (volatile uint32_t *)(uintptr_t)dma_buffer_vaddr;

    for (size_t i = 0; i < DMA_TEST_BYTES / sizeof(uint32_t); i++) {
        buf[i] = 0x5a000000u | (uint32_t)i;
        buf[(DMA_RETURN_OFFSET / sizeof(uint32_t)) + i] = 0;
    }
}

static bool check_dma_return_buffer(void)
{
    volatile uint32_t *buf = (volatile uint32_t *)(uintptr_t)dma_buffer_vaddr;

    for (size_t i = 0; i < DMA_TEST_BYTES / sizeof(uint32_t); i++) {
        uint32_t expected = 0x5a000000u | (uint32_t)i;
        uint32_t actual = buf[(DMA_RETURN_OFFSET / sizeof(uint32_t)) + i];
        if (actual != expected) {
            microkit_dbg_puts("DMA return buffer mismatch at word ");
            put_hex64(i);
            microkit_dbg_puts(": expected ");
            put_hex64(expected);
            microkit_dbg_puts(", got ");
            put_hex64(actual);
            microkit_dbg_puts("\n");
            return false;
        }
    }
    return true;
}

static bool check_edu_liveness(uintptr_t edu_base)
{
    const uint32_t probe = 0xa5a55a5au;

    edu_write32(edu_base, EDU_REG_ALIVE, probe);
    return edu_read32(edu_base, EDU_REG_ALIVE) == ~probe;
}

void init(void)
{
    microkit_dbg_puts("x86_64_iommu_dma_test: starting\n");

    uint32_t id = pci_config_read32(EDU_BUS, EDU_DEV, EDU_FUNC, 0x00);
    if ((id & 0xFFFFu) != EDU_PCI_VENDOR_ID || ((id >> 16) & 0xFFFFu) != EDU_PCI_DEVICE_ID) {
        microkit_dbg_puts("x86_64_iommu_dma_test: EDU PCI device not found, id=");
        put_hex64(id);
        microkit_dbg_puts("\n");
        return;
    }

    pci_config_write32(EDU_BUS, EDU_DEV, EDU_FUNC, PCI_BAR0, EDU_MMIO_PADDR);

    uint64_t bar0 = pci_config_read32(EDU_BUS, EDU_DEV, EDU_FUNC, PCI_BAR0) & ~0xfu;
    if (bar0 < EDU_MMIO_PADDR || bar0 + EDU_BAR_SIZE > EDU_MMIO_PADDR + EDU_MMIO_SIZE) {
        microkit_dbg_puts("x86_64_iommu_dma_test: EDU BAR outside mapped MMIO window, bar=");
        put_hex64(bar0);
        microkit_dbg_puts("\n");
        return;
    }

    uint16_t command = pci_config_read16(EDU_BUS, EDU_DEV, EDU_FUNC, PCI_COMMAND);
    command |= PCI_COMMAND_MEMORY | PCI_COMMAND_MASTER;
    pci_config_write16(EDU_BUS, EDU_DEV, EDU_FUNC, PCI_COMMAND, command);

    uintptr_t edu_base = (uintptr_t)edu_mmio_window_vaddr + (uintptr_t)(bar0 - EDU_MMIO_PADDR);
    microkit_dbg_puts("x86_64_iommu_dma_test: EDU BAR=");
    put_hex64(bar0);
    microkit_dbg_puts(", id_reg=");
    uint32_t device_id = edu_read32(edu_base, EDU_REG_ID);
    put_hex64(device_id);
    if ((device_id & 0xFF) != EDU_DEVICE_ID) {
        microkit_dbg_puts("x86_64_iommu_dma_test: Identification failed.\n");
        return;
    }
    microkit_dbg_puts("\n");

    if (!check_edu_liveness(edu_base)) {
        microkit_dbg_puts("x86_64_iommu_dma_test: EDU MMIO liveness check failed\n");
        return;
    }

    /* The qemu education device works by copying from DRAM to an internal device buffer or vice-versa. */
    fill_dma_buffer();

    edu_write64(edu_base, EDU_REG_DMA_SRC, DMA_BUFFER_IOVA);
    edu_write64(edu_base, EDU_REG_DMA_DST, EDU_DMA_SRAM);
    edu_write64(edu_base, EDU_REG_DMA_COUNT, DMA_TEST_BYTES);
    edu_write32(edu_base, EDU_REG_DMA_CMD, EDU_DMA_START);

    wait_for_dma(edu_base);

    edu_write64(edu_base, EDU_REG_DMA_SRC, EDU_DMA_SRAM);
    edu_write64(edu_base, EDU_REG_DMA_DST, DMA_BUFFER_IOVA + DMA_RETURN_OFFSET);
    edu_write64(edu_base, EDU_REG_DMA_COUNT, DMA_TEST_BYTES);
    edu_write32(edu_base, EDU_REG_DMA_CMD, EDU_DMA_START | EDU_DMA_TO_RAM);

    wait_for_dma(edu_base);

    if (!check_dma_return_buffer()) {
        microkit_dbg_puts("x86_64_iommu_dma_test: DMA verification failed\n");
        return;
    }

    microkit_dbg_puts("x86_64_iommu_dma_test: DMA verification passed\n");
}

void notified(microkit_channel ch)
{
    (void)ch;
}
