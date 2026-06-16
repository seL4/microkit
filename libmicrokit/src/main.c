/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include <sel4/sel4.h>

#include <microkit.h>

#define INPUT_CAP 1
#define REPLY_CAP 4

#define PD_MASK 0xff
#define CHANNEL_MASK 0x3f

#define BADGE_FAULT_BIT 62
#define BADGE_ENDPOINT_BIT 63

/* All globals are prefixed with microkit_* to avoid clashes with user defined globals. */

bool microkit_passive;
char microkit_name[MICROKIT_PD_NAME_LENGTH];
/* We use seL4 typedefs as this variable is exposed to the libmicrokit header
 * and we do not want to rely on compiler built-in defines. */
seL4_Bool microkit_have_signal = seL4_False;
seL4_CPtr microkit_signal_cap;
seL4_MessageInfo_t microkit_signal_msg;

#if defined(CONFIG_VTX)
struct microkit_x86_vcpu_state microkit_x86_vcpu_state;
#endif /* CONFIG_VTX */

seL4_Word microkit_irqs;
seL4_Word microkit_notifications;
seL4_Word microkit_pps;
seL4_Word microkit_ioports;

extern seL4_IPCBuffer __sel4_ipc_buffer_obj;

seL4_IPCBuffer *__sel4_ipc_buffer = &__sel4_ipc_buffer_obj;

extern const void (*const __init_array_start [])(void);
extern const void (*const __init_array_end [])(void);

__attribute__((weak)) microkit_msginfo protected(microkit_channel ch, microkit_msginfo msginfo)
{
    microkit_dbg_puts(microkit_name);
    microkit_dbg_puts(" is missing the 'protected' entry point\n");
    microkit_internal_crash(0);
    return seL4_MessageInfo_new(0, 0, 0, 0);
}

__attribute__((weak)) seL4_Bool fault(microkit_child child, microkit_msginfo msginfo, microkit_msginfo *reply_msginfo)
{
    microkit_dbg_puts(microkit_name);
    microkit_dbg_puts(" is missing the 'fault' entry point\n");
    microkit_internal_crash(0);
    return seL4_False;
}

static void run_init_funcs(void)
{
    size_t count = __init_array_end - __init_array_start;
    for (size_t i = 0; i < count; i++) {
        __init_array_start[i]();
    }
}

static void deferred_flush(void)
{
    if (microkit_have_signal) {
        seL4_Send(microkit_signal_cap, microkit_signal_msg);
        microkit_have_signal = seL4_False;
    }
}

#if defined(CONFIG_VTX)
static seL4_MessageInfo_t x86_vcpu_resume(seL4_Word *badge)
{
    /* There is no seL4 invocation which combines a VMEnter and a non-blocking send.
     * Thus, we must perform any deferred signals from `microkit_deferred_notify()` /
     * `microkit_deferred_irq_ack()` before invoking VMEnter. */
    deferred_flush();

    seL4_Word is_fault, fault_reason;
    struct microkit_x86_vcpu_state *s = &microkit_x86_vcpu_state;

    x64_sys_send_recv(seL4_SysVMEnter, 0, badge, 0, &is_fault,
                      &s->rip, &s->prim_proc_ctl, &s->irq_info, &fault_reason, 0);
    /* We want to follow the documented kernel behaviour where these 4 values are
     * always populated in the message registers. However, due to `setMR()`'s behaviour
     * in include/object/tcb.h of the kernel, these 4 values will only be set in
     * CPU registers. So we need to copy them to the IPC buffer to conform to the
     * documented kernel behaviour.
     *
     * The other reason why we want to manage these values is so that the user does not
     * have to worry about saving/updating them when servicing a notification. */
    microkit_mr_set(SEL4_VMENTER_CALL_EIP_MR, s->rip);
    microkit_mr_set(SEL4_VMENTER_CALL_CONTROL_PPC_MR, s->prim_proc_ctl);
    microkit_mr_set(SEL4_VMENTER_CALL_INTERRUPT_INFO_MR, s->irq_info);
    /* The 4th value (SEL4_VMENTER_FAULT_REASON_MR) is only valid when is_fault is true. */

    /* Create a dummy msgInfo so that we can call `fault()`, as on x86 a VMExit is
     * not an IPC as on other architectures. */
    if (is_fault) {
        microkit_mr_set(SEL4_VMENTER_FAULT_REASON_MR, fault_reason);
        *badge |= 1ull << BADGE_FAULT_BIT;
        return seL4_MessageInfo_new(0, 0, 0, SEL4_VMENTER_NUM_FAULT_MSGS);
    } else {
        /* VCPU got interrupted due to a notification, no msgInfo. */
        return seL4_MessageInfo_new(0, 0, 0, 0);
    }
}
#endif

static void handler_loop(void)
{
    bool have_reply = false;
    seL4_MessageInfo_t reply_tag = seL4_MessageInfo_new(0, 0, 0, 0);

    /**
     * Because of https://github.com/seL4/seL4/issues/1536
     * let's acknowledge all the IRQs after we've started.
     */
    {
        seL4_Word irqs_to_ack = microkit_irqs;
        unsigned int idx = 0;
        do {
            if (irqs_to_ack & 1) {
                microkit_irq_ack(idx);
            }

            irqs_to_ack >>= 1;
            idx++;
        } while (irqs_to_ack != 0);
    }

    for (;;) {
        seL4_Word badge;
        seL4_MessageInfo_t tag;

#if defined(CONFIG_VTX)
        if (microkit_x86_vcpu_state.is_on) {
            /* We should never have a reply message from the `protected()` endpoint,
            * as on x86 a PD with a bound vCPU cannot receive PPCs.*/
            // assert(!have_reply);
            tag = x86_vcpu_resume(&badge);
        } else if (have_reply) {
#else
        if (have_reply) {
#endif
            deferred_flush();
            tag = seL4_ReplyRecv(INPUT_CAP, reply_tag, &badge, REPLY_CAP);
        } else if (microkit_have_signal) {
            tag = seL4_NBSendRecv(microkit_signal_cap, microkit_signal_msg, INPUT_CAP, &badge, REPLY_CAP);
            microkit_have_signal = seL4_False;
        } else {
            tag = seL4_Recv(INPUT_CAP, &badge, REPLY_CAP);
        }

        uint64_t is_endpoint = badge >> BADGE_ENDPOINT_BIT;
        uint64_t is_fault = (badge >> BADGE_FAULT_BIT) & 1;

        have_reply = false;

        if (is_fault) {
            seL4_Bool reply_to_fault = fault(badge & PD_MASK, tag, &reply_tag);
#if defined(CONFIG_VTX)
            /* If fault() returns false then we shouldn't resume the VCPU. */
            if (!reply_to_fault) {
                microkit_x86_vcpu_state.is_on = seL4_False;
            }
            /* There won't be anything to reply to for a VCPU fault. */
            reply_to_fault = seL4_False;
#endif
            if (reply_to_fault) {
                have_reply = true;
            }
        } else if (is_endpoint) {
            have_reply = true;
            reply_tag = protected(badge & CHANNEL_MASK, tag);
        } else {
            unsigned int idx = 0;
            do  {
                if (badge & 1) {
                    notified(idx);
                }
                badge >>= 1;
                idx++;
            } while (badge != 0);
        }
    }
}

void main(void)
{
    run_init_funcs();
    init();

    /*
     * If we are passive, now our initialisation is complete we can
     * signal the monitor to unbind our scheduling context and bind
     * it to our notification object.
     * We delay this signal so we are ready waiting on a recv() syscall
     */
    if (microkit_passive) {
        microkit_have_signal = seL4_True;
        microkit_signal_msg = seL4_MessageInfo_new(0, 0, 0, 0);
        microkit_signal_cap = MONITOR_EP;
    }

    handler_loop();
}
