#pragma once

#include <stdint.h>

/* Definitions for the PL011 UART. Adjust the base address as required. */
uintptr_t uart_base_vaddr;

#define UART_IRQ_CH 1

#define RHR_MASK               0b111111111
#define UARTDR                 0x000
#define UARTFR                 0x018
#define UARTIMSC               0x038
#define UARTICR                0x044
#define PL011_UARTFR_TXFF      (1 << 5)
#define PL011_UARTFR_RXFE      (1 << 4)

#define REG_PTR(base, offset)  ((volatile uint32_t *)((base) + (offset)))

static void uart_init(void) {
    *REG_PTR(uart_base_vaddr, UARTIMSC) = 0x50;
}

static int uart_get_char(void) {
    int ch = 0;
    if ((*REG_PTR(uart_base_vaddr, UARTFR) & PL011_UARTFR_RXFE) == 0) {
        ch = *REG_PTR(uart_base_vaddr, UARTDR) & RHR_MASK;
    }
    switch (ch) {
    case '\n':
        ch = '\r';
        break;
    case 8:
        ch = 0x7f;
        break;
    }
    return ch;
}

static void uart_put_char(int ch) {
    while ((*REG_PTR(uart_base_vaddr, UARTFR) & PL011_UARTFR_TXFF) != 0);
    *REG_PTR(uart_base_vaddr, UARTDR) = ch;
    if (ch == '\r') {
        uart_put_char('\n');
    }
}

static void uart_handle_irq(void) {
    *REG_PTR(uart_base_vaddr, UARTICR) = 0x7f0;
}

static void uart_put_str(char *str) {
    while (*str) {
        uart_put_char(*str);
        str++;
    }
}

static void print_num(uint64_t num) {
    if (num == 0) {
        uart_put_char('0');
        return;
    }

    if (num > 9) {
        print_num(num / 10);
    }
    uart_put_char('0' + (num % 10));
}