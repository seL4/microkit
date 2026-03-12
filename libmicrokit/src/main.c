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

/* All globals are prefixed with microkit_* to avoid clashes with user defined globals. */

bool microkit_passive;
char microkit_name[MICROKIT_PD_NAME_LENGTH];
/* We use seL4 typedefs as this variable is exposed to the libmicrokit header
 * and we do not want to rely on compiler built-in defines. */
seL4_Bool microkit_have_signal = seL4_False;
seL4_CPtr microkit_signal_cap;
seL4_MessageInfo_t microkit_signal_msg;

#if defined(CONFIG_VTX)
seL4_Bool microkit_x86_do_vcpu_resume = seL4_False;
seL4_Word microkit_x86_vcpu_resume_rip;
seL4_Word microkit_x86_vcpu_resume_ppc; /* "Primary Processor-Based VM-Execution Controls" */
seL4_Word microkit_x86_vcpu_resume_irq; /* "VM-Entry Interruption-Information Field" */
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

static seL4_MessageInfo_t receive_event(bool have_reply, seL4_MessageInfo_t reply_tag, seL4_Word *badge)
{
    if (have_reply) {
        return seL4_ReplyRecv(INPUT_CAP, reply_tag, badge, REPLY_CAP);
    } else if (microkit_have_signal) {
        return seL4_NBSendRecv(microkit_signal_cap, microkit_signal_msg, INPUT_CAP, badge, REPLY_CAP);
        microkit_have_signal = seL4_False;
    } else {
        return seL4_Recv(INPUT_CAP, badge, REPLY_CAP);
    }
}

static void handler_loop(void)
{
    bool have_reply = false;
    seL4_MessageInfo_t reply_tag;

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
        seL4_Word x86_vmenter_result;
        seL4_Bool x86_vmenter_result_valid = seL4_False;
        if (microkit_x86_do_vcpu_resume) {
            /* There isn't a syscall that atomically send signal or reply, perform vmenter while waiting for
               incoming notifications. So we have to dispatch any signal or reply first. Then switch execution
               to the bound VCPU. */
            if (have_reply) {
                seL4_Send(REPLY_CAP, reply_tag);
            } else if (microkit_have_signal) {
                seL4_NBSend(microkit_signal_cap, microkit_signal_msg);
            }

            microkit_mr_set(SEL4_VMENTER_CALL_EIP_MR, microkit_x86_vcpu_resume_rip);
            microkit_mr_set(SEL4_VMENTER_CALL_CONTROL_PPC_MR, microkit_x86_vcpu_resume_ppc);
            microkit_mr_set(SEL4_VMENTER_CALL_INTERRUPT_INFO_MR, microkit_x86_vcpu_resume_irq);

            x86_vmenter_result = seL4_VMEnter(&badge);

            x86_vmenter_result_valid = seL4_True;
            microkit_x86_do_vcpu_resume = seL4_False;
        } else {
            tag = receive_event(have_reply, reply_tag, &badge);
        }
#else
        tag = receive_event(have_reply, reply_tag, &badge);
#endif /* CONFIG_VTX */

        uint64_t is_endpoint = badge >> 63;
        uint64_t is_fault = (badge >> 62) & 1;

#if defined(CONFIG_VTX)
        if (x86_vmenter_result_valid) {
            if (x86_vmenter_result == SEL4_VMENTER_RESULT_FAULT) {
                is_fault = 1;
                tag = seL4_MessageInfo_new(0, 0, 0, 0);
            } else if (x86_vmenter_result == SEL4_VMENTER_RESULT_NOTIF) {
                /* VCPU was interrupted while executing from a notification. Saves the instruction pointer,
                 * Primary Processor-Based VM-Execution Controls and VM-Entry Interruption-Information Field.
                 * So we can resume automatically with the correct values on the next event loop iteration. */
                microkit_vcpu_x86_deferred_resume(microkit_mr_get(SEL4_VMENTER_CALL_EIP_MR), microkit_mr_get(SEL4_VMENTER_CALL_CONTROL_PPC_MR), microkit_mr_get(SEL4_VMENTER_CALL_INTERRUPT_INFO_MR));
            }
        }
#endif /* CONFIG_VTX */

        have_reply = false;

        if (is_fault) {
            seL4_Bool reply_to_fault = fault(badge & PD_MASK, tag, &reply_tag);

#if defined(CONFIG_VTX)
            if (x86_vmenter_result_valid == seL4_False && reply_to_fault == seL4_True) {
                /* Fault from child PD rather than from bound VCPU */
                have_reply = true;
            }
#else
            if (reply_to_fault == seL4_True) {
                have_reply = true;
            }
#endif /* CONFIG_VTX */

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
