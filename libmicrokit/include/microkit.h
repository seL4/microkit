/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

/* Microkit interface */

#pragma once

#include <stdint.h>
#include <stdbool.h>

#define __thread
#include <sel4/sel4.h>

typedef unsigned int microkit_channel;
typedef seL4_MessageInfo_t microkit_msginfo;

#define MONITOR_EP 5
#define BASE_OUTPUT_NOTIFICATION_CAP 10
#define BASE_ENDPOINT_CAP 74
#define BASE_IRQ_CAP 138

#define MICROKIT_MAX_CHANNELS 63

/* User provided functions */
void init(void);
void notified(microkit_channel ch);
microkit_msginfo protected(microkit_channel ch, microkit_msginfo msginfo);

extern char microkit_name[16];
/* These next three variables are so our PDs can combine a signal with the next Recv syscall */
extern bool have_signal;
extern seL4_CPtr signal;
extern seL4_MessageInfo_t signal_msg;

/*
 * Output a single character on the debug console.
 */
void microkit_dbg_putc(int c);

/*
 * Output a NUL terminated string to the debug console.
 */
void microkit_dbg_puts(const char *s);

static inline void microkit_notify(microkit_channel ch)
{
    seL4_Signal(BASE_OUTPUT_NOTIFICATION_CAP + ch);
}

static inline void microkit_irq_ack(microkit_channel ch)
{
    seL4_IRQHandler_Ack(BASE_IRQ_CAP + ch);
}

static inline microkit_msginfo microkit_ppcall(microkit_channel ch, microkit_msginfo msginfo)
{
    return seL4_Call(BASE_ENDPOINT_CAP + ch, msginfo);
}

static inline microkit_msginfo microkit_msginfo_new(uint64_t label, uint16_t count)
{
    return seL4_MessageInfo_new(label, 0, 0, count);
}

static inline uint64_t microkit_msginfo_get_label(microkit_msginfo msginfo)
{
    return seL4_MessageInfo_get_label(msginfo);
}

static void microkit_mr_set(uint8_t mr, uint64_t value)
{
    seL4_SetMR(mr, value);
}

static uint64_t microkit_mr_get(uint8_t mr)
{
    return seL4_GetMR(mr);
}
