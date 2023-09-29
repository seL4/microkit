/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

/* Microkit interface */

#pragma once

#include <stdbool.h>
#include <stdint.h>

#define __thread
#include <sel4/sel4.h>

typedef unsigned int microkit_channel;
typedef unsigned int microkit_id;
typedef seL4_MessageInfo_t microkit_msginfo;

#define REPLY_CAP 4
#define MONITOR_ENDPOINT_CAP 5
#define TCB_CAP 6
#define SMC_CAP 7
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
void fault(microkit_channel ch, microkit_msginfo msginfo);

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

// @ivanv: When building a non-optimised build of something that uses the library, doing something like seL4_UserContext ctx = {0} does not work...
// Figure out why it doesn't
static inline void
memzero(void *s, unsigned long n)
{
    uint8_t *p;
    for (p = (uint8_t *)s; n > 0; n--, p++) {
        *p = 0;
    }
}

static inline void
microkit_internal_crash(seL4_Error err)
{
    /*
     * Currently crash be dereferencing NULL page
     *
     * Actually derference 'err' which means the crash reporting will have
     * `err` as the fault address. A bit of a cute hack. Not a good long term
     * solution but good for now.
     */
    int *x = (int *)(uintptr_t) err;
    *x = 0;
}

static inline void
microkit_notify(microkit_channel ch)
{
    seL4_Signal(BASE_OUTPUT_NOTIFICATION_CAP + ch);
}

static inline void
microkit_irq_ack(microkit_channel ch)
{
    seL4_IRQHandler_Ack(BASE_IRQ_CAP + ch);
}

/*
 * Note that microkit_notify_delayed and microkit_irq_ack_delayed are experimental
 * functions that allow a notify/signal or IRQ ack to happen when we get back
 * into the Microkit event handler loop while only making one syscall. This can
 * improve performance as this will cause an NBSendRecv to occur in the handler
 * loop, meaning that you avoid an extra context switch into the kernel
 * compared to if you were to do a regular microkit_notify or microkit_irq_ack.
 *
 * Whether these functions should become part of mainline libmicrokit API is yet
 * to be discussed.
 */
static inline void
microkit_notify_delayed(microkit_channel ch)
{
    have_signal = true;
    signal_msg = seL4_MessageInfo_new(0, 0, 0, 0);
    signal = (BASE_OUTPUT_NOTIFICATION_CAP + ch);
}

static inline void
microkit_irq_ack_delayed(microkit_channel ch)
{
    have_signal = true;
    signal_msg = seL4_MessageInfo_new(IRQAckIRQ, 0, 0, 0);
    signal = (BASE_IRQ_CAP + ch);
}

static inline void
microkit_pd_restart(microkit_id pd, uintptr_t entry_point)
{
    seL4_Error err;
    seL4_UserContext ctxt;
    memzero(&ctxt, sizeof(seL4_UserContext));
    ctxt.pc = entry_point;
    err = seL4_TCB_WriteRegisters(
        BASE_TCB_CAP + pd,
        true,
        0, /* No flags */
        1, /* writing 1 register */
        &ctxt
    );

    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_pd_restart: error writing TCB registers\n");
        microkit_internal_crash(err);
    }
}

static inline void
microkit_pd_stop(microkit_id pd)
{
    seL4_Error err;
    err = seL4_TCB_Suspend(BASE_TCB_CAP + pd);
    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_pd_stop: error suspending TCB\n");
        microkit_internal_crash(err);
    }
}

static inline void
microkit_fault_reply(microkit_msginfo msginfo)
{
    // @ivanv: revisit
    seL4_Send(REPLY_CAP, msginfo);
}

static inline microkit_msginfo
microkit_ppcall(microkit_channel ch, microkit_msginfo msginfo)
{
    return seL4_Call(BASE_ENDPOINT_CAP + ch, msginfo);
}

static inline microkit_msginfo
microkit_msginfo_new(uint64_t label, uint16_t count)
{
    return seL4_MessageInfo_new(label, 0, 0, count);
}

static inline uint64_t
microkit_msginfo_get_label(microkit_msginfo msginfo)
{
    return seL4_MessageInfo_get_label(msginfo);
}

static void
microkit_mr_set(uint8_t mr, uint64_t value)
{
    seL4_SetMR(mr, value);
}

static uint64_t
microkit_mr_get(uint8_t mr)
{
    return seL4_GetMR(mr);
}

#if defined(CONFIG_ARM_HYPERVISOR_SUPPORT) || defined(CONFIG_RISCV_HYPERVISOR_SUPPORT)
static inline void
// @ivanv: the implementation of this is exactly the same as microkit_pd_restart (same
// with pd_stop and vm_stop). Potentially could just use one.
microkit_vm_restart(microkit_id vm, uintptr_t entry_point)
{
    seL4_Error err;
    seL4_UserContext ctxt;
    memzero(&ctxt, sizeof(seL4_UserContext));
    ctxt.pc = entry_point;
    err = seL4_TCB_WriteRegisters(
        BASE_VM_TCB_CAP + vm,
        true,
        0, /* No flags */
        1, /* writing 1 register */
        &ctxt
    );

    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_vm_restart: error writing registers\n");
        microkit_internal_crash(err);
    }
}

static inline void
microkit_vm_stop(microkit_id vm)
{
    seL4_Error err;
    err = seL4_TCB_Suspend(BASE_VM_TCB_CAP + vm);
    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_vm_stop: error suspending TCB\n");
        microkit_internal_crash(err);
    }
}
#endif

// @ivanv: Ideally these functions like vcpu_read_reg would be architecture
// independent from the user's perspective.
/* Wrappers over ARM specific hypervisor system calls. */
#if defined(CONFIG_ARM_HYPERVISOR_SUPPORT)
static inline void
microkit_arm_vcpu_inject_irq(microkit_id vm, uint16_t irq, uint8_t priority, uint8_t group, uint8_t index)
{
    seL4_Error err;
    err = seL4_ARM_VCPU_InjectIRQ(BASE_VCPU_CAP + vm, irq, priority, group, index);
    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_arm_vcpu_inject_irq: error injecting IRQ\n");
        microkit_internal_crash(err);
    }
}

static inline void
microkit_arm_vcpu_ack_vppi(microkit_id vm, uint64_t irq)
{
    seL4_Error err;
    err = seL4_ARM_VCPU_AckVPPI(BASE_VCPU_CAP + vm, irq);
    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_arm_vcpu_ack_vppi: error acking VPPI\n");
        microkit_internal_crash(err);
    }
}

static inline seL4_Word
microkit_arm_vcpu_read_reg(microkit_id vm, uint64_t reg)
{
    seL4_ARM_VCPU_ReadRegs_t ret;
    ret = seL4_ARM_VCPU_ReadRegs(BASE_VCPU_CAP + vm, reg);
    if (ret.error != seL4_NoError) {
        microkit_dbg_puts("microkit_arm_vcpu_read_reg: error reading VCPU register\n");
        microkit_internal_crash(ret.error);
    }

    return ret.value;
}

static inline void
microkit_arm_vcpu_write_reg(microkit_id vm, uint64_t reg, uint64_t value)
{
    seL4_Error err;
    err = seL4_ARM_VCPU_WriteRegs(BASE_VCPU_CAP + vm, reg, value);
    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_arm_vcpu_write_reg: error VPPI\n");
        microkit_internal_crash(err);
    }
}
#endif

#if defined(CONFIG_RISCV_HYPERVISOR_SUPPORT)
static inline seL4_Word
microkit_riscv_vcpu_read_reg(microkit_id vm, uint64_t reg)
{
    seL4_RISCV_VCPU_ReadRegs_t ret;
    ret = seL4_RISCV_VCPU_ReadRegs(BASE_VCPU_CAP + vm, reg);
    if (ret.error != seL4_NoError) {
        microkit_dbg_puts("microkit_riscv_vcpu_read_reg: error reading VCPU register\n");
        microkit_internal_crash(ret.error);
    }

    return ret.value;
}

static inline void
microkit_riscv_vcpu_write_reg(microkit_id vm, uint64_t reg, uint64_t value)
{
    seL4_Error err;
    err = seL4_RISCV_VCPU_WriteRegs(BASE_VCPU_CAP + vm, reg, value);
    if (err != seL4_NoError) {
        microkit_dbg_puts("microkit_riscv_vcpu_write_reg: error VPPI\n");
        microkit_internal_crash(err);
    }
}
#endif
