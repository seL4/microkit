/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

/* Microkit interface */

#pragma once

#define __thread
#include <sel4/sel4.h>

typedef unsigned int microkit_channel;
typedef unsigned int microkit_pd;
typedef seL4_MessageInfo_t microkit_msginfo;

#define MONITOR_EP 5
#define BASE_OUTPUT_NOTIFICATION_CAP 10
#define BASE_ENDPOINT_CAP 74
#define BASE_IRQ_CAP 138
#define BASE_TCB_CAP 202

#define MICROKIT_MAX_CHANNELS 62

/* User provided functions */
void init(void);
void notified(microkit_channel ch);
microkit_msginfo protected(microkit_channel ch, microkit_msginfo msginfo);
void fault(microkit_pd pd, microkit_msginfo msginfo);

extern char microkit_name[16];
/* These next three variables are so our PDs can combine a signal with the next Recv syscall */
extern seL4_Bool microkit_have_signal;
extern seL4_CPtr microkit_signal_cap;
extern seL4_MessageInfo_t microkit_signal_msg;

/*
 * Output a single character on the debug console.
 */
void microkit_dbg_putc(int c);

/*
 * Output a NUL terminated string to the debug console.
 */
void microkit_dbg_puts(const char *s);

static inline void microkit_internal_crash(seL4_Error err)
{
    /*
     * Currently crash be dereferencing NULL page
     *
     * Actually derference 'err' which means the crash reporting will have
     * `err` as the fault address. A bit of a cute hack. Not a good long term
     * solution but good for now.
     */
    int *x = (int *)(seL4_Word) err;
    *x = 0;
}

static inline void microkit_notify(microkit_channel ch)
{
    seL4_Signal(BASE_OUTPUT_NOTIFICATION_CAP + ch);
}

static inline void microkit_irq_ack(microkit_channel ch)
{
    seL4_IRQHandler_Ack(BASE_IRQ_CAP + ch);
}

static inline void microkit_pd_restart(microkit_pd pd, seL4_Word entry_point)
{
    seL4_Error err;
    seL4_UserContext ctxt = {0};
    ctxt.pc = entry_point;
    err = seL4_TCB_WriteRegisters(
              BASE_TCB_CAP + pd,
              seL4_True,
              0, /* No flags */
              1, /* writing 1 register */
              &ctxt
          );

    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_pd_restart: error writing TCB registers\n");
        microkit_internal_crash(err);
    }
}

static inline void microkit_pd_stop(microkit_pd pd)
{
    seL4_Error err;
    err = seL4_TCB_Suspend(BASE_TCB_CAP + pd);
    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_pd_stop: error writing TCB registers\n");
        microkit_internal_crash(err);
    }
}

static inline microkit_msginfo microkit_ppcall(microkit_channel ch, microkit_msginfo msginfo)
{
    return seL4_Call(BASE_ENDPOINT_CAP + ch, msginfo);
}

static inline microkit_msginfo microkit_msginfo_new(seL4_Word label, seL4_Uint16 count)
{
    return seL4_MessageInfo_new(label, 0, 0, count);
}

static inline seL4_Word microkit_msginfo_get_label(microkit_msginfo msginfo)
{
    return seL4_MessageInfo_get_label(msginfo);
}

static inline seL4_Word microkit_msginfo_get_count(microkit_msginfo msginfo)
{
    return seL4_MessageInfo_get_length(msginfo);
}

static void microkit_mr_set(seL4_Uint8 mr, seL4_Word value)
{
    seL4_SetMR(mr, value);
}

static seL4_Word microkit_mr_get(seL4_Uint8 mr)
{
    return seL4_GetMR(mr);
}
