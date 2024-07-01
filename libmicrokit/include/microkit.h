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
typedef unsigned int microkit_child;
typedef seL4_MessageInfo_t microkit_msginfo;

#define MONITOR_EP 5
#define BASE_OUTPUT_NOTIFICATION_CAP 10
#define BASE_ENDPOINT_CAP 74
#define BASE_IRQ_CAP 138
#define BASE_TCB_CAP 202
#define BASE_VM_TCB_CAP 266
#define BASE_VCPU_CAP 330

#define MICROKIT_MAX_CHANNELS 62

/* User provided functions */
void init(void);
void notified(microkit_channel ch);
microkit_msginfo protected(microkit_channel ch, microkit_msginfo msginfo);
seL4_Bool fault(microkit_child child, microkit_msginfo msginfo, microkit_msginfo *reply_msginfo);

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

static inline void microkit_pd_restart(microkit_child pd, seL4_Word entry_point)
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

static inline void microkit_pd_stop(microkit_child pd)
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

/* The following APIs are only available where the kernel is built as a hypervisor. */
#if defined(CONFIG_ARM_HYPERVISOR_SUPPORT)
static inline void microkit_vcpu_restart(microkit_child vcpu, seL4_Word entry_point)
{
    seL4_Error err;
    seL4_UserContext ctxt = {0};
    ctxt.pc = entry_point;
    err = seL4_TCB_WriteRegisters(
              BASE_VM_TCB_CAP + vcpu,
              seL4_True,
              0, /* No flags */
              1, /* writing 1 register */
              &ctxt
          );

    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_vm_restart: error writing registers\n");
        microkit_internal_crash(err);
    }
}

static inline void microkit_vcpu_stop(microkit_child vcpu)
{
    seL4_Error err;
    err = seL4_TCB_Suspend(BASE_VM_TCB_CAP + vcpu);
    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_vm_stop: error suspending TCB\n");
        microkit_internal_crash(err);
    }
}

static inline void microkit_vcpu_arm_inject_irq(microkit_child vcpu, seL4_Uint16 irq, seL4_Uint8 priority,
                                                seL4_Uint8 group, seL4_Uint8 index)
{
    seL4_Error err;
    err = seL4_ARM_VCPU_InjectIRQ(BASE_VCPU_CAP + vcpu, irq, priority, group, index);
    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_arm_vcpu_inject_irq: error injecting IRQ\n");
        microkit_internal_crash(err);
    }
}

static inline void microkit_vcpu_arm_ack_vppi(microkit_child vcpu, seL4_Word irq)
{
    seL4_Error err;
    err = seL4_ARM_VCPU_AckVPPI(BASE_VCPU_CAP + vcpu, irq);
    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_arm_vcpu_ack_vppi: error acking VPPI\n");
        microkit_internal_crash(err);
    }
}

static inline seL4_Word microkit_vcpu_arm_read_reg(microkit_child vcpu, seL4_Word reg)
{
    seL4_ARM_VCPU_ReadRegs_t ret;
    ret = seL4_ARM_VCPU_ReadRegs(BASE_VCPU_CAP + vcpu, reg);
    if (ret.error != seL4_NoError) {
        microkit_dbg_puts("microkit_arm_vcpu_read_reg: error reading vCPU register\n");
        microkit_internal_crash(ret.error);
    }

    return ret.value;
}

static inline void microkit_vcpu_arm_write_reg(microkit_child vcpu, seL4_Word reg, seL4_Word value)
{
    seL4_Error err;
    err = seL4_ARM_VCPU_WriteRegs(BASE_VCPU_CAP + vcpu, reg, value);
    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_arm_vcpu_write_reg: error writing vCPU register\n");
        microkit_internal_crash(err);
    }
}
#endif
