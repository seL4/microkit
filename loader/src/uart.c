/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include "uart.h"

#if defined(CONFIG_PRINTING)

#define UART_REG(x) ((volatile uint32_t *)(UART_BASE + (x)))

static void putc(uint8_t ch);

#if defined(CONFIG_PLAT_TQMA8XQP1GB)
#define UART_BASE 0x5a070000
#define STAT 0x14
#define TRANSMIT 0x1c
#define STAT_TDRE (1 << 23)

void uart_init(void) {}

void putc(uint8_t ch)
{
    while (!(*UART_REG(STAT) & STAT_TDRE)) { }
    *UART_REG(TRANSMIT) = ch;
}

#elif defined(CONFIG_PLAT_IMX8MM_EVK) || defined(CONFIG_PLAT_IMX8MP_EVK)
/* IMX8MP_EVK includes the iotgate, as this is the kernel platform */
#define UART_BASE 0x30890000
#define STAT 0x98
#define TRANSMIT 0x40
#define STAT_TDRE (1 << 14)

void uart_init(void) {}

void putc(uint8_t ch)
{
    while (!(*UART_REG(STAT) & STAT_TDRE)) { }
    *UART_REG(TRANSMIT) = ch;
}
#elif defined(CONFIG_PLAT_ZYNQMP_ZCU102)
#define UART_BASE 0xff000000
#define UART_CHANNEL_STS_TXEMPTY 0x8
#define UART_CHANNEL_STS         0x2C
#define UART_TX_RX_FIFO          0x30

#define UART_CR             0x00
#define UART_CR_TX_EN       (1 << 4)
#define UART_CR_TX_DIS      (1 << 5)

void uart_init(void)
{
    uint32_t ctrl = *UART_REG(UART_CR);
    ctrl |= UART_CR_TX_EN;
    ctrl &= ~UART_CR_TX_DIS;
    *UART_REG(UART_CR) = ctrl;
}

void putc(uint8_t ch)
{
    while (!(*UART_REG(UART_CHANNEL_STS) & UART_CHANNEL_STS_TXEMPTY));
    *UART_REG(UART_TX_RX_FIFO) = ch;
}
#elif defined(CONFIG_PLAT_MAAXBOARD) || defined(CONFIG_PLAT_IMX8MQ_EVK)
#define UART_BASE 0x30860000
#define STAT 0x98
#define TRANSMIT 0x40
#define STAT_TDRE (1 << 14)

void uart_init(void) {}

void putc(uint8_t ch)
{
    // ensure FIFO has space
    while (!(*UART_REG(STAT) & STAT_TDRE)) { }
    *UART_REG(TRANSMIT) = ch;
}
#elif defined(CONFIG_PLAT_ODROIDC2)
#define UART_BASE 0xc81004c0
#define UART_WFIFO 0x0
#define UART_STATUS 0xC
#define UART_TX_FULL (1 << 21)

void uart_init(void) {}

void putc(uint8_t ch)
{
    while ((*UART_REG(UART_STATUS) & UART_TX_FULL));
    *UART_REG(UART_WFIFO) = ch;
}
#elif defined(CONFIG_PLAT_ODROIDC4)
#define UART_BASE 0xff803000
#define UART_WFIFO 0x0
#define UART_STATUS 0xC
#define UART_TX_FULL (1 << 21)

void uart_init(void) {}

void putc(uint8_t ch)
{
    while ((*UART_REG(UART_STATUS) & UART_TX_FULL));
    *UART_REG(UART_WFIFO) = ch;
}
#elif defined(CONFIG_PLAT_ZYNQMP_ULTRA96V2)
/* Use UART1 available through USB-to-JTAG/UART pod */
#define UART_BASE 0x00ff010000
#define R_UART_CHANNEL_STS          0x2C
#define UART_CHANNEL_STS_TXEMPTY    0x08
#define UART_CHANNEL_STS_TACTIVE    0x800
#define R_UART_TX_RX_FIFO           0x30

void uart_init(void) {}

void putc(uint8_t ch)
{
    while (!(*UART_REG(R_UART_CHANNEL_STS) & UART_CHANNEL_STS_TXEMPTY)) {};
    while (*UART_REG(R_UART_CHANNEL_STS) & UART_CHANNEL_STS_TACTIVE) {};

    *((volatile uint32_t *)(UART_BASE + R_UART_TX_RX_FIFO)) = ch;
}
#elif defined(CONFIG_PLAT_QEMU_ARM_VIRT)
#define UART_BASE                 0x9000000
#define PL011_TCR                 0x030
#define PL011_UARTDR              0x000
#define PL011_UARTFR              0x018
#define PL011_UARTFR_TXFF         (1 << 5)
#define PL011_CR_UART_EN          (1 << 0)
#define PL011_CR_TX_EN            (1 << 8)

void uart_init(void)
{
    /* Enable the device and transmit */
    *UART_REG(PL011_TCR) |= (PL011_CR_TX_EN | PL011_CR_UART_EN);
}

void putc(uint8_t ch)
{
    while ((*UART_REG(PL011_UARTFR) & PL011_UARTFR_TXFF) != 0);
    *UART_REG(PL011_UARTDR) = ch;
}

#elif defined(CONFIG_PLAT_BCM2711)
/* rpi4b */
#define UART_BASE 0xfe215040
#define MU_IO 0x00
#define MU_LSR 0x14
#define MU_LSR_TXIDLE (1 << 6)

void uart_init(void) {}

void putc(uint8_t ch)
{
    while (!(*UART_REG(MU_LSR) & MU_LSR_TXIDLE));
    *UART_REG(MU_IO) = (ch & 0xff);
}
#elif defined(CONFIG_PLAT_ROCKPRO64)
#define UART_BASE   0xff1a0000
#define UTHR        0x0
#define ULSR        0x14
#define ULSR_THRE   (1 << 5)

void uart_init(void) {}

void putc(uint8_t ch)
{
    while ((*UART_REG(ULSR) & ULSR_THRE) == 0);
    *UART_REG(UTHR) = ch;
}

#elif defined(CONFIG_ARCH_RISCV64)

#include "riscv/sbi.h"

void uart_init(void)
{
    /* Nothing to do, OpenSBI will do UART init for us. */
}

void putc(uint8_t ch)
{
    sbi_call(SBI_EXT_DEBUG_CONSOLE, SBI_DEBUG_CONSOLE_WRITE_BYTE, ch, 0, 0, 0, 0, 0);
}
#else
#error Board not defined
#endif

void puts(const char *s)
{
    while (*s) {
        if (*s == '\n') {
            putc('\r');
        }
        putc(*s);
        s++;
    }
}

static inline char hexchar(unsigned int v)
{
    return v < 10 ? '0' + v : ('a' - 10) + v;
}

void puthex32(uint32_t val)
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

void puthex64(uint64_t val)
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

#endif /* CONFIG_PRINTING */
