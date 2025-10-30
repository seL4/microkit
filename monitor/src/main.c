/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
/*
 * The Microkit Monitor.
 *
 * The monitor is the highest priority Protection Domain
 * exclusively in a Microkit system. It fulfills one purpose:
 *
 *   Acting as the fault handler for protection domains.
 */

/*
 * Why this you may ask? Well, the seL4 headers depend on
 * a global `__sel4_ipc_buffer` which is a pointer to the
 * thread's IPC buffer. Which is reasonable enough, passing
 * that explicitly to every function would be annoying.
 *
 * The seL4 headers make this global a thread-local global,
 * which is also reasonable, considering it applies to a
 * specific thread! But, for our purposes we don't have threads!
 *
 * Thread local storage is painful and annoying to configure.
 * We'd really rather NOT use thread local storage (especially
 * consider we never have more than one thread in a Vspace)
 *
 * So, by defining __thread to be empty it means the variable
 * becomes a true global rather than thread local storage
 * variable, which means, we don't need to waste a bunch
 * of effort and complexity on thread local storage implementation.
 */
#define __thread

#include <stdbool.h>
#include <stdint.h>
#include <sel4/sel4.h>

#include "util.h"

#define MAX_VMS 64
#define MAX_PDS 64
#define MAX_NAME_LEN 64

#define FAULT_EP_CAP 1
#define REPLY_CAP 2
#define BASE_PD_TCB_CAP 10
#define BASE_VM_TCB_CAP 74
#define BASE_SCHED_CONTEXT_CAP 138
#define BASE_NOTIFICATION_CAP 202

extern seL4_IPCBuffer __sel4_ipc_buffer_obj;
seL4_IPCBuffer *__sel4_ipc_buffer = &__sel4_ipc_buffer_obj;

char pd_names[MAX_PDS][MAX_NAME_LEN];
seL4_Word pd_names_len;
char vm_names[MAX_VMS][MAX_NAME_LEN] __attribute__((unused));
seL4_Word vm_names_len;

/* For reporting potential stack overflows, keep track of the stack regions for each PD. */
seL4_Word pd_stack_bottom_addrs[MAX_PDS];

/* Sanity check that the architecture specific macro have been set. */
#if defined(ARCH_aarch64)
#elif defined(ARCH_x86_64)
#elif defined(ARCH_riscv64)
#else
#error "No architecture flag was defined, double check your CC flags"
#endif

#ifdef ARCH_riscv64
/*
 * Convert the fault status register given by the kernel into a string describing
 * what fault happened. The FSR is the 'scause' register.
 */
static char *riscv_fsr_to_string(seL4_Word fsr)
{
    switch (fsr) {
    case 0:
        return "Instruction address misaligned";
    case 1:
        return "Instruction access fault";
    case 2:
        return "Illegal instruction";
    case 3:
        return "Breakpoint";
    case 4:
        return "Load address misaligned";
    case 5:
        return "Load access fault";
    case 6:
        return "Store/AMO address misaligned";
    case 7:
        return "Store/AMO access fault";
    case 8:
        return "Environment call from U-mode";
    case 9:
        return "Environment call from S-mode";
    case 12:
        return "Instruction page fault";
    case 13:
        return "Load page fault";
    case 15:
        return "Store/AMO page fault";
    case 18:
        return "Software check";
    case 19:
        return "Hardware error";
    default:
        return "<Unexpected FSR>";
    }
}
#endif

#ifdef ARCH_aarch64
static char *ec_to_string(uintptr_t ec)
{
    switch (ec) {
    case 0:
        return "Unknown reason";
    case 1:
        return "Trapped WFI or WFE instruction execution";
    case 3:
        return "Trapped MCR or MRC access with (coproc==0b1111) this is not reported using EC 0b000000";
    case 4:
        return "Trapped MCRR or MRRC access with (coproc==0b1111) this is not reported using EC 0b000000";
    case 5:
        return "Trapped MCR or MRC access with (coproc==0b1110)";
    case 6:
        return "Trapped LDC or STC access";
    case 7:
        return "Access to SVC, Advanced SIMD or floating-point functionality trapped";
    case 12:
        return "Trapped MRRC access with (coproc==0b1110)";
    case 13:
        return "Branch Target Exception";
    case 17:
        return "SVC instruction execution in AArch32 state";
    case 21:
        return "SVC instruction execution in AArch64 state";
    case 24:
        return "Trapped MSR, MRS or System instruction exuection in AArch64 state, this is not reported using EC 0xb000000, 0b000001 or 0b000111";
    case 25:
        return "Access to SVE functionality trapped";
    case 28:
        return "Exception from a Pointer Authentication instruction authentication failure";
    case 32:
        return "Instruction Abort from a lower Exception level";
    case 33:
        return "Instruction Abort taken without a change in Exception level";
    case 34:
        return "PC alignment fault exception";
    case 36:
        return "Data Abort from a lower Exception level";
    case 37:
        return "Data Abort taken without a change in Exception level";
    case 38:
        return "SP alignment faultr exception";
    case 40:
        return "Trapped floating-point exception taken from AArch32 state";
    case 44:
        return "Trapped floating-point exception taken from AArch64 state";
    case 47:
        return "SError interrupt";
    case 48:
        return "Breakpoint exception from a lower Exception level";
    case 49:
        return "Breakpoint exception taken without a change in Exception level";
    case 50:
        return "Software Step exception from a lower Exception level";
    case 51:
        return "Software Step exception taken without a change in Exception level";
    case 52:
        return "Watchpoint exception from a lower Exception level";
    case 53:
        return "Watchpoint exception taken without a change in Exception level";
    case 56:
        return "BKPT instruction execution in AArch32 state";
    case 60:
        return "BRK instruction execution in AArch64 state";
    }
    return "<invalid EC>";
}

static char *data_abort_dfsc_to_string(uintptr_t dfsc)
{
    switch (dfsc) {
    case 0x00:
        return "address size fault, level 0";
    case 0x01:
        return "address size fault, level 1";
    case 0x02:
        return "address size fault, level 2";
    case 0x03:
        return "address size fault, level 3";
    case 0x04:
        return "translation fault, level 0";
    case 0x05:
        return "translation fault, level 1";
    case 0x06:
        return "translation fault, level 2";
    case 0x07:
        return "translation fault, level 3";
    case 0x09:
        return "access flag fault, level 1";
    case 0x0a:
        return "access flag fault, level 2";
    case 0x0b:
        return "access flag fault, level 3";
    case 0x0d:
        return "permission fault, level 1";
    case 0x0e:
        return "permission fault, level 2";
    case 0x0f:
        return "permission fault, level 3";
    case 0x10:
        return "synchronuos external abort";
    case 0x11:
        return "synchronous tag check fault";
    case 0x14:
        return "synchronous external abort, level 0";
    case 0x15:
        return "synchronous external abort, level 1";
    case 0x16:
        return "synchronous external abort, level 2";
    case 0x17:
        return "synchronous external abort, level 3";
    case 0x18:
        return "synchronous parity or ECC error";
    case 0x1c:
        return "synchronous parity or ECC error, level 0";
    case 0x1d:
        return "synchronous parity or ECC error, level 1";
    case 0x1e:
        return "synchronous parity or ECC error, level 2";
    case 0x1f:
        return "synchronous parity or ECC error, level 3";
    case 0x21:
        return "alignment fault";
    case 0x30:
        return "tlb conflict abort";
    case 0x31:
        return "unsupported atomic hardware update fault";
    }
    return "<unexpected DFSC>";
}
#endif

#ifdef ARCH_x86_64
static char *page_fault_to_string(seL4_Word fsr)
{
    // https://wiki.osdev.org/Exceptions#Page_Fault
    switch (fsr) {
    case 0 | 4:
        return "read to a non-present page at ring 3";
    case 1 | 4:
        return "page-protection violation from read at ring 3";
    case 2 | 4:
        return "write to a non-present page at ring 3";
    case 3 | 4:
        return "page-protection violation from write at ring 3";
    case 16:
        // Note that seL4 currently does not implement the NX/XD bit
        // to mark a page as non-executable so we will never see the below message.
        return "instruction fetch from non-executable page";
    default:
        return "invalid FSR or unimplemented decoding";
    }
}
#endif

/* UBSAN decoding related functionality */
#define UBSAN_ARM64_BRK_IMM 0x5500
#define UBSAN_ARM64_BRK_MASK 0x00ff
#define ESR_COMMENT_MASK ((1 << 16) - 1)
#define ARM64_BRK_EC 60

/*
 * ABI defined by Clang's UBSAN enum SanitizerHandler:
 * https://github.com/llvm/llvm-project/blob/release/16.x/clang/lib/CodeGen/CodeGenFunction.h#L113
 */
enum UBSAN_CHECKS {
    UBSAN_ADD_OVERFLOW,
    UBSAN_BUILTIN_UNREACHABLE,
    UBSAN_CFI_CHECK_FAIL,
    UBSAN_DIVREM_OVERFLOW,
    UBSAN_DYNAMIC_TYPE_CACHE_MISS,
    UBSAN_FLOAT_CAST_OVERFLOW,
    UBSAN_FUNCTION_TYPE_MISMATCH,
    UBSAN_IMPLICIT_CONVERSION,
    UBSAN_INVALID_BUILTIN,
    UBSAN_INVALID_OBJC_CAST,
    UBSAN_LOAD_INVALID_VALUE,
    UBSAN_MISSING_RETURN,
    UBSAN_MUL_OVERFLOW,
    UBSAN_NEGATE_OVERFLOW,
    UBSAN_NULLABILITY_ARG,
    UBSAN_NULLABILITY_RETURN,
    UBSAN_NONNULL_ARG,
    UBSAN_NONNULL_RETURN,
    UBSAN_OUT_OF_BOUNDS,
    UBSAN_POINTER_OVERFLOW,
    UBSAN_SHIFT_OUT_OF_BOUNDS,
    UBSAN_SUB_OVERFLOW,
    UBSAN_TYPE_MISMATCH,
    UBSAN_ALIGNMENT_ASSUMPTION,
    UBSAN_VLA_BOUND_NOT_POSITIVE,
};

#ifdef CONFIG_ARM_HYPERVISOR_SUPPORT
static char *usban_code_to_string(seL4_Word code)
{
    switch (code) {
    case UBSAN_ADD_OVERFLOW:
        return "add overflow";
    case UBSAN_BUILTIN_UNREACHABLE:
        return "builtin unreachable";
    case UBSAN_CFI_CHECK_FAIL:
        return "control-flow-integrity check fail";
    case UBSAN_DIVREM_OVERFLOW:
        return "division remainder overflow";
    case UBSAN_DYNAMIC_TYPE_CACHE_MISS:
        return "dynamic type cache miss";
    case UBSAN_FLOAT_CAST_OVERFLOW:
        return "float case overflow";
    case UBSAN_FUNCTION_TYPE_MISMATCH:
        return "function type mismatch";
    case UBSAN_IMPLICIT_CONVERSION:
        return "implicit conversion";
    case UBSAN_INVALID_BUILTIN:
        return "invalid builtin";
    case UBSAN_INVALID_OBJC_CAST:
        return "invalid objc cast";
    case UBSAN_LOAD_INVALID_VALUE:
        return "load invalid value";
    case UBSAN_MISSING_RETURN:
        return "missing return";
    case UBSAN_MUL_OVERFLOW:
        return "multiplication overflow";
    case UBSAN_NEGATE_OVERFLOW:
        return "negate overflow";
    case UBSAN_NULLABILITY_ARG:
        return "nullability argument";
    case UBSAN_NULLABILITY_RETURN:
        return "nullability return";
    case UBSAN_NONNULL_ARG:
        return "non-null argument";
    case UBSAN_NONNULL_RETURN:
        return "non-null return";
    case UBSAN_OUT_OF_BOUNDS:
        return "out of bounds access";
    case UBSAN_POINTER_OVERFLOW:
        return "pointer overflow";
    case UBSAN_SHIFT_OUT_OF_BOUNDS:
        return "shift out of bounds";
    case UBSAN_SUB_OVERFLOW:
        return "subtraction overflow";
    case UBSAN_TYPE_MISMATCH:
        return "type mismatch";
    case UBSAN_ALIGNMENT_ASSUMPTION:
        return "alignment assumption";
    case UBSAN_VLA_BOUND_NOT_POSITIVE:
        return "variable-length-array bound not positive";
    default:
        return "unknown reason";
    }
}
#endif

static void print_tcb_registers(seL4_UserContext *regs)
{
#if defined(ARCH_riscv64)
    puts("Registers: \n");
    puts("pc : ");
    puthex64(regs->pc);
    puts("\n");
    puts("ra : ");
    puthex64(regs->ra);
    puts("\n");
    puts("s0 : ");
    puthex64(regs->s0);
    puts("\n");
    puts("s1 : ");
    puthex64(regs->s1);
    puts("\n");
    puts("s2 : ");
    puthex64(regs->s2);
    puts("\n");
    puts("s3 : ");
    puthex64(regs->s3);
    puts("\n");
    puts("s4 : ");
    puthex64(regs->s4);
    puts("\n");
    puts("s5 : ");
    puthex64(regs->s5);
    puts("\n");
    puts("s6 : ");
    puthex64(regs->s6);
    puts("\n");
    puts("s7 : ");
    puthex64(regs->s7);
    puts("\n");
    puts("s8 : ");
    puthex64(regs->s8);
    puts("\n");
    puts("s9 : ");
    puthex64(regs->s9);
    puts("\n");
    puts("s10 : ");
    puthex64(regs->s10);
    puts("\n");
    puts("s11 : ");
    puthex64(regs->s11);
    puts("\n");
    puts("a0 : ");
    puthex64(regs->a0);
    puts("\n");
    puts("a1 : ");
    puthex64(regs->a1);
    puts("\n");
    puts("a2 : ");
    puthex64(regs->a2);
    puts("\n");
    puts("a3 : ");
    puthex64(regs->a3);
    puts("\n");
    puts("a4 : ");
    puthex64(regs->a4);
    puts("\n");
    puts("a5 : ");
    puthex64(regs->a5);
    puts("\n");
    puts("a6 : ");
    puthex64(regs->a6);
    puts("\n");
    puts("t0 : ");
    puthex64(regs->t0);
    puts("\n");
    puts("t1 : ");
    puthex64(regs->t1);
    puts("\n");
    puts("t2 : ");
    puthex64(regs->t2);
    puts("\n");
    puts("t3 : ");
    puthex64(regs->t3);
    puts("\n");
    puts("t4 : ");
    puthex64(regs->t4);
    puts("\n");
    puts("t5 : ");
    puthex64(regs->t5);
    puts("\n");
    puts("t6 : ");
    puthex64(regs->t6);
    puts("\n");
    puts("tp : ");
    puthex64(regs->tp);
    puts("\n");
#elif defined(ARCH_aarch64)
    puts("Registers: \n");
    puts("pc : ");
    puthex64(regs->pc);
    puts("\n");
    puts("sp: ");
    puthex64(regs->sp);
    puts("\n");
    puts("spsr : ");
    puthex64(regs->spsr);
    puts("\n");
    puts("x0 : ");
    puthex64(regs->x0);
    puts("\n");
    puts("x1 : ");
    puthex64(regs->x1);
    puts("\n");
    puts("x2 : ");
    puthex64(regs->x2);
    puts("\n");
    puts("x3 : ");
    puthex64(regs->x3);
    puts("\n");
    puts("x4 : ");
    puthex64(regs->x4);
    puts("\n");
    puts("x5 : ");
    puthex64(regs->x5);
    puts("\n");
    puts("x6 : ");
    puthex64(regs->x6);
    puts("\n");
    puts("x7 : ");
    puthex64(regs->x7);
    puts("\n");
    puts("x8 : ");
    puthex64(regs->x8);
    puts("\n");
    puts("x16 : ");
    puthex64(regs->x16);
    puts("\n");
    puts("x17 : ");
    puthex64(regs->x17);
    puts("\n");
    puts("x18 : ");
    puthex64(regs->x18);
    puts("\n");
    puts("x29 : ");
    puthex64(regs->x29);
    puts("\n");
    puts("x30 : ");
    puthex64(regs->x30);
    puts("\n");
    puts("x9 : ");
    puthex64(regs->x9);
    puts("\n");
    puts("x10 : ");
    puthex64(regs->x10);
    puts("\n");
    puts("x11 : ");
    puthex64(regs->x11);
    puts("\n");
    puts("x12 : ");
    puthex64(regs->x12);
    puts("\n");
    puts("x13 : ");
    puthex64(regs->x13);
    puts("\n");
    puts("x14 : ");
    puthex64(regs->x14);
    puts("\n");
    puts("x15 : ");
    puthex64(regs->x15);
    puts("\n");
    puts("x19 : ");
    puthex64(regs->x19);
    puts("\n");
    puts("x20 : ");
    puthex64(regs->x20);
    puts("\n");
    puts("x21 : ");
    puthex64(regs->x21);
    puts("\n");
    puts("x22 : ");
    puthex64(regs->x22);
    puts("\n");
    puts("x23 : ");
    puthex64(regs->x23);
    puts("\n");
    puts("x24 : ");
    puthex64(regs->x24);
    puts("\n");
    puts("x25 : ");
    puthex64(regs->x25);
    puts("\n");
    puts("x26 : ");
    puthex64(regs->x26);
    puts("\n");
    puts("x27 : ");
    puthex64(regs->x27);
    puts("\n");
    puts("x28 : ");
    puthex64(regs->x28);
    puts("\n");
    puts("tpidr_el0 : ");
    puthex64(regs->tpidr_el0);
    puts("\n");
    puts("tpidrro_el0 : ");
    puthex64(regs->tpidrro_el0);
    puts("\n");
#elif ARCH_x86_64
    puts("Registers: \n");
    puts("rip : ");
    puthex64(regs->rip);
    puts("\n");
    puts("rsp: ");
    puthex64(regs->rsp);
    puts("\n");
    puts("rflags : ");
    puthex64(regs->rflags);
    puts("\n");
    puts("rax : ");
    puthex64(regs->rax);
    puts("\n");
    puts("rbx : ");
    puthex64(regs->rbx);
    puts("\n");
    puts("rcx : ");
    puthex64(regs->rcx);
    puts("\n");
    puts("rdx : ");
    puthex64(regs->rdx);
    puts("\n");
    puts("rsi : ");
    puthex64(regs->rsi);
    puts("\n");
    puts("rdi : ");
    puthex64(regs->rdi);
    puts("\n");
    puts("rbp : ");
    puthex64(regs->rbp);
    puts("\n");
    puts("r8 : ");
    puthex64(regs->r8);
    puts("\n");
    puts("r9 : ");
    puthex64(regs->r9);
    puts("\n");
    puts("r10 : ");
    puthex64(regs->r10);
    puts("\n");
    puts("r11 : ");
    puthex64(regs->r11);
    puts("\n");
    puts("r12 : ");
    puthex64(regs->r12);
    puts("\n");
    puts("r13 : ");
    puthex64(regs->r13);
    puts("\n");
    puts("r14 : ");
    puthex64(regs->r14);
    puts("\n");
    puts("r15 : ");
    puthex64(regs->r15);
    puts("\n");
    puts("fs_base : ");
    puthex64(regs->fs_base);
    puts("\n");
    puts("gs_base : ");
    puthex64(regs->gs_base);
    puts("\n");
#endif
}

#ifdef ARCH_riscv64
static void riscv_print_vm_fault()
{
    seL4_Word ip = seL4_GetMR(seL4_VMFault_IP);
    seL4_Word fault_addr = seL4_GetMR(seL4_VMFault_Addr);
    seL4_Word is_instruction = seL4_GetMR(seL4_VMFault_PrefetchFault);
    seL4_Word fsr = seL4_GetMR(seL4_VMFault_FSR);
    puts("MON|ERROR: VMFault: ip=");
    puthex64(ip);
    puts("  fault_addr=");
    puthex64(fault_addr);
    puts("  fsr=");
    puthex64(fsr);
    puts("  ");
    puts(is_instruction ? "(instruction fault)" : "(data fault)");
    puts("\n");
    puts("MON|ERROR: description of fault: ");
    puts(riscv_fsr_to_string(fsr));
    puts("\n");
}
#endif

#if ARCH_x86_64
static void x86_64_print_vm_fault()
{
    seL4_Word ip = seL4_GetMR(seL4_VMFault_IP);
    seL4_Word fault_addr = seL4_GetMR(seL4_VMFault_Addr);
    seL4_Word is_instruction = seL4_GetMR(seL4_VMFault_PrefetchFault);
    seL4_Word fsr = seL4_GetMR(seL4_VMFault_FSR);
    puts("MON|ERROR: VMFault: ip=");
    puthex64(ip);
    puts("  fault_addr=");
    puthex64(fault_addr);
    puts("  fsr=");
    puthex64(fsr);
    puts("  ");
    puts(is_instruction ? "(instruction fault)" : "(data fault)");
    puts("\n");

    puts("MON|ERROR: description of fault: ");
    puts(page_fault_to_string(fsr));
    puts("\n");
}
#endif

#ifdef ARCH_aarch64
static void aarch64_print_vm_fault()
{
    seL4_Word ip = seL4_GetMR(seL4_VMFault_IP);
    seL4_Word fault_addr = seL4_GetMR(seL4_VMFault_Addr);
    seL4_Word is_instruction = seL4_GetMR(seL4_VMFault_PrefetchFault);
    seL4_Word fsr = seL4_GetMR(seL4_VMFault_FSR);
    seL4_Word ec = fsr >> 26;
    seL4_Word il = fsr >> 25 & 1;
    seL4_Word iss = fsr & 0x1ffffffUL;
    puts("MON|ERROR: VMFault: ip=");
    puthex64(ip);
    puts("  fault_addr=");
    puthex64(fault_addr);
    puts("  fsr=");
    puthex64(fsr);
    puts("  ");
    puts(is_instruction ? "(instruction fault)" : "(data fault)");
    puts("\n");
    puts("MON|ERROR:   ec: ");
    puthex32(ec);
    puts("  ");
    puts(ec_to_string(ec));
    puts("   il: ");
    puts(il ? "1" : "0");
    puts("   iss: ");
    puthex32(iss);
    puts("\n");

    if (ec == 0x24) {
        /* FIXME: Note, this is not a complete decoding of the fault! Just some of the more
           common fields!
        */
        seL4_Word dfsc = iss & 0x3f;
        bool ea = (iss >> 9) & 1;
        bool cm = (iss >> 8) & 1;
        bool s1ptw = (iss >> 7) & 1;
        bool wnr = (iss >> 6) & 1;
        puts("MON|ERROR:   dfsc = ");
        puts(data_abort_dfsc_to_string(dfsc));
        puts(" (");
        puthex32(dfsc);
        puts(")");
        if (ea) {
            puts(" -- external abort");
        }
        if (cm) {
            puts(" -- cache maint");
        }
        if (s1ptw) {
            puts(" -- stage 2 fault for stage 1 page table walk");
        }
        if (wnr) {
            puts(" -- write not read");
        }
        puts("\n");
    }
}
#endif

static void monitor(void)
{
    for (;;) {
        seL4_Word badge, label;
        seL4_MessageInfo_t tag;
        seL4_Error err;

        tag = seL4_Recv(FAULT_EP_CAP, &badge, REPLY_CAP);
        label = seL4_MessageInfo_get_label(tag);

        seL4_Word pd_id = badge - 1;
        seL4_Word tcb_cap = BASE_PD_TCB_CAP + pd_id;

        if (label == seL4_Fault_NullFault && pd_id < MAX_PDS) {
            /* This is a request from our PD to become passive */
            err = seL4_SchedContext_UnbindObject(BASE_SCHED_CONTEXT_CAP + pd_id, tcb_cap);
            err = seL4_SchedContext_Bind(BASE_SCHED_CONTEXT_CAP + pd_id, BASE_NOTIFICATION_CAP + pd_id);
            if (err != seL4_NoError) {
                puts("MON|ERROR: could not bind scheduling context to notification object");
            } else {
                puts("MON|INFO: PD '");
                puts(pd_names[pd_id]);
                puts("' is now passive!\n");
            }
            continue;
        }

        puts("MON|ERROR: received message ");
        puthex32(label);
        puts("  badge: ");
        puthex64(badge);
        puts("  tcb cap: ");
        puthex64(tcb_cap);
        puts("\n");

        if (pd_id < MAX_PDS && pd_names[pd_id][0] != 0) {
            puts("MON|ERROR: faulting PD: ");
            puts(pd_names[pd_id]);
            puts("\n");
        } else {
            fail("MON|ERROR: unknown/invalid badge\n");
        }

        seL4_UserContext regs;

        err = seL4_TCB_ReadRegisters(tcb_cap, false, 0, sizeof(seL4_UserContext) / sizeof(seL4_Word), &regs);
        if (err != seL4_NoError) {
            fail("error reading registers");
        }

        print_tcb_registers(&regs);

        switch (label) {
        case seL4_Fault_CapFault: {
            seL4_Word ip = seL4_GetMR(seL4_CapFault_IP);
            seL4_Word fault_addr = seL4_GetMR(seL4_CapFault_Addr);
            seL4_Word in_recv_phase = seL4_GetMR(seL4_CapFault_InRecvPhase);
            seL4_Word lookup_failure_type = seL4_GetMR(seL4_CapFault_LookupFailureType);
            seL4_Word bits_left = seL4_GetMR(seL4_CapFault_BitsLeft);
            seL4_Word depth_bits_found = seL4_GetMR(seL4_CapFault_DepthMismatch_BitsFound);
            seL4_Word guard_found = seL4_GetMR(seL4_CapFault_GuardMismatch_GuardFound);
            seL4_Word guard_bits_found = seL4_GetMR(seL4_CapFault_GuardMismatch_BitsFound);

            puts("MON|ERROR: CapFault: ip=");
            puthex64(ip);
            puts("  fault_addr=");
            puthex64(fault_addr);
            puts("  in_recv_phase=");
            puts(in_recv_phase == 0 ? "false" : "true");
            puts("  lookup_failure_type=");

            switch (lookup_failure_type) {
            case seL4_NoFailure:
                puts("seL4_NoFailure");
                break;
            case seL4_InvalidRoot:
                puts("seL4_InvalidRoot");
                break;
            case seL4_MissingCapability:
                puts("seL4_MissingCapability");
                break;
            case seL4_DepthMismatch:
                puts("seL4_DepthMismatch");
                break;
            case seL4_GuardMismatch:
                puts("seL4_GuardMismatch");
                break;
            default:
                puthex64(lookup_failure_type);
            }

            if (
                lookup_failure_type == seL4_MissingCapability ||
                lookup_failure_type == seL4_DepthMismatch ||
                lookup_failure_type == seL4_GuardMismatch) {
                puts("  bits_left=");
                puthex64(bits_left);
            }
            if (lookup_failure_type == seL4_DepthMismatch) {
                puts("  depth_bits_found=");
                puthex64(depth_bits_found);
            }
            if (lookup_failure_type == seL4_GuardMismatch) {
                puts("  guard_found=");
                puthex64(guard_found);
                puts("  guard_bits_found=");
                puthex64(guard_bits_found);
            }
            puts("\n");
            break;
        }
        case seL4_Fault_UserException: {
            puts("MON|ERROR: UserException\n");
            break;
        }
        case seL4_Fault_VMFault: {
#if defined(ARCH_aarch64)
            aarch64_print_vm_fault();
#elif defined(ARCH_riscv64)
            riscv_print_vm_fault();
#elif defined(ARCH_x86_64)
            x86_64_print_vm_fault();
#else
#error "Unknown architecture to print a VM fault for"
#endif

            seL4_Word fault_addr = seL4_GetMR(seL4_VMFault_Addr);
            seL4_Word stack_addr = pd_stack_bottom_addrs[pd_id];
            if (fault_addr < stack_addr && fault_addr >= stack_addr - 0x1000) {
                puts("MON|ERROR: potential stack overflow, fault address within one page outside of stack region\n");
            }

            break;
        }
#ifdef CONFIG_ARM_HYPERVISOR_SUPPORT
        case seL4_Fault_VCPUFault: {
            seL4_Word esr = seL4_GetMR(seL4_VCPUFault_HSR);
            seL4_Word ec = esr >> 26;

            puts("MON|ERROR: received vCPU fault with ESR: ");
            puthex64(esr);
            puts("\n");

            seL4_Word esr_comment = esr & ESR_COMMENT_MASK;
            if (ec == ARM64_BRK_EC && ((esr_comment & ~UBSAN_ARM64_BRK_MASK) == UBSAN_ARM64_BRK_IMM)) {
                /* We likely have a UBSAN check going off from a brk instruction */
                seL4_Word ubsan_code = esr_comment & UBSAN_ARM64_BRK_MASK;
                puts("MON|ERROR: potential undefined behaviour detected by UBSAN for: '");
                puts(usban_code_to_string(ubsan_code));
                puts("'\n");
            } else {
                puts("MON|ERROR: Unknown vCPU fault\n");
            }
            break;
        }
#endif
        default:
            puts("MON|ERROR: Unknown fault\n");
            puthex64(label);
            break;
        }
    }
}

void main(void)
{
#if CONFIG_DEBUG_BUILD
    /*
     * Assign PD/VM names to each TCB with seL4, this helps debugging when an error
     * message is printed by seL4 or if we dump the scheduler state.
     */
    for (unsigned idx = 0; idx < pd_names_len; idx++) {
        seL4_DebugNameThread(BASE_PD_TCB_CAP + idx, pd_names[idx]);
    }
    for (unsigned idx = 0; idx < vm_names_len; idx++) {
        seL4_DebugNameThread(BASE_VM_TCB_CAP + idx, vm_names[idx]);
    }
#endif

    puts("MON|INFO: Microkit Monitor started!\n");

    monitor();
}
