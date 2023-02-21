/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
/*
 * The seL4 Core Platform Monitor.
 *
 * The monitor is the initial task in a core platform system.
 *
 * The monitor fulfills two purposes:
 *
 *   1. creating the initial state of the system.
 *   2. acting as the fault handler for for protection domains.
 *
 * Initialisation is performed by executing a number of kernel
 * invocations to create and configure kernel objects.
 *
 * The specific invocations to make are configured by the build
 * tool; the monitor simply reads a data structure to execute
 * each invocation one at a time.
 *
 * The process occurs in a two step manner. The first bootstrap
 * step execute the `bootstrap_invocations` only. The purpose
 * of this bootstrap is to get the system to the point for the
 * `system_invocations` is mapped into the monitors address space.
 * Once this occurs it is possible for the monitor to switch to
 * executing invocation from this second data structure.
 *
 * The motivation for this design is to keep both the initial
 * task image and the initial CNode as small, fixed size entities.
 *
 * Fixed size allows both kernel and monitor to avoid unnecesary
 * recompilation for different system configurations. Keeping things
 * small optimizes overall memory usage.
 *
 *
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
#include "debug.h"

#define MAX_PDS 64
#define MAX_NAME_LEN 16
#define MAX_TCBS 64

#define MAX_UNTYPED_REGIONS 256

/* Max words available for bootstrap invocations.
 *
 * Only a small number of syscalls is required to
 * get to the point where the main syscalls data
 * is mapped in, so we keep this small.
 *
 * FIXME: This can be smaller once compression is enabled.
 */
#define BOOTSTRAP_INVOCATION_DATA_SIZE 150

seL4_IPCBuffer *__sel4_ipc_buffer;

char _stack[4096];

static char pd_names[MAX_PDS][MAX_NAME_LEN];

seL4_Word fault_ep;
seL4_Word reply;
seL4_Word tcbs[MAX_TCBS];
seL4_Word scheduling_contexts[MAX_TCBS];
seL4_Word notification_caps[MAX_TCBS];

struct region {
    uintptr_t paddr;
    uintptr_t size_bits;
    uintptr_t is_device; /*FIXME: should back size_bits / is_device */
};

struct untyped_info {
    seL4_Word cap_start;
    seL4_Word cap_end;
    struct region regions[MAX_UNTYPED_REGIONS];
};

seL4_Word bootstrap_invocation_count;
seL4_Word bootstrap_invocation_data[BOOTSTRAP_INVOCATION_DATA_SIZE];

seL4_Word system_invocation_count;
seL4_Word *system_invocation_data = (void*)0x80000000;

struct untyped_info untyped_info;

static char *
ec_to_string(uintptr_t ec)
{
    switch (ec) {
        case 0: return "Unknown reason";
        case 1: return "Trapped WFI or WFE instruction execution";
        case 3: return "Trapped MCR or MRC access with (coproc==0b1111) this is not reported using EC 0b000000";
        case 4: return "Trapped MCRR or MRRC access with (coproc==0b1111) this is not reported using EC 0b000000";
        case 5: return "Trapped MCR or MRC access with (coproc==0b1110)";
        case 6: return "Trapped LDC or STC access";
        case 7: return "Access to SVC, Advanced SIMD or floating-point functionality trapped";
        case 12: return "Trapped MRRC access with (coproc==0b1110)";
        case 13: return "Branch Target Exception";
        case 17: return "SVC instruction execution in AArch32 state";
        case 21: return "SVC instruction execution in AArch64 state";
        case 24: return "Trapped MSR, MRS or System instruction exuection in AArch64 state, this is not reported using EC 0xb000000, 0b000001 or 0b000111";
        case 25: return "Access to SVE functionality trapped";
        case 28: return "Exception from a Pointer Authentication instruction authentication failure";
        case 32: return "Instruction Abort from a lower Exception level";
        case 33: return "Instruction Abort taken without a change in Exception level";
        case 34: return "PC alignment fault exception";
        case 36: return "Data Abort from a lower Exception level";
        case 37: return "Data Abort taken without a change in Exception level";
        case 38: return "SP alignment faultr exception";
        case 40: return "Trapped floating-point exception taken from AArch32 state";
        case 44: return "Trapped floating-point exception taken from AArch64 state";
        case 47: return "SError interrupt";
        case 48: return "Breakpoint exception from a lower Exception level";
        case 49: return "Breakpoint exception taken without a change in Exception level";
        case 50: return "Software Step exception from a lower Exception level";
        case 51: return "Software Step exception taken without a change in Exception level";
        case 52: return "Watchpoint exception from a lower Exception level";
        case 53: return "Watchpoint exception taken without a change in Exception level";
        case 56: return "BKPT instruction execution in AArch32 state";
        case 60: return "BRK instruction execution in AArch64 state";
    }
    return "<invalid EC>";
}

static char *
data_abort_dfsc_to_string(uintptr_t dfsc)
{
    switch(dfsc) {
        case 0x00: return "address size fault, level 0";
        case 0x01: return "address size fault, level 1";
        case 0x02: return "address size fault, level 2";
        case 0x03: return "address size fault, level 3";
        case 0x04: return "translation fault, level 0";
        case 0x05: return "translation fault, level 1";
        case 0x06: return "translation fault, level 2";
        case 0x07: return "translation fault, level 3";
        case 0x09: return "access flag fault, level 1";
        case 0x0a: return "access flag fault, level 2";
        case 0x0b: return "access flag fault, level 3";
        case 0x0d: return "permission fault, level 1";
        case 0x0e: return "permission fault, level 2";
        case 0x0f: return "permission fault, level 3";
        case 0x10: return "synchronuos external abort";
        case 0x11: return "synchronous tag check fault";
        case 0x14: return "synchronous external abort, level 0";
        case 0x15: return "synchronous external abort, level 1";
        case 0x16: return "synchronous external abort, level 2";
        case 0x17: return "synchronous external abort, level 3";
        case 0x18: return "syncrhonous partity or ECC error";
        case 0x1c: return "syncrhonous partity or ECC error, level 0";
        case 0x1d: return "syncrhonous partity or ECC error, level 1";
        case 0x1e: return "syncrhonous partity or ECC error, level 2";
        case 0x1f: return "syncrhonous partity or ECC error, level 3";
        case 0x21: return "alignment fault";
        case 0x30: return "tlb conflict abort";
        case 0x31: return "unsupported atomic hardware update fault";
    }
    return "<unexpected DFSC>";
}

static void
check_untypeds_match(seL4_BootInfo *bi)
{
    /* Check that untypeds list generate from build matches the kernel */
    if (untyped_info.cap_start != bi->untyped.start) {
        puts("MON|ERROR: cap start mismatch. Expected cap start: ");
        puthex32(untyped_info.cap_start);
        puts("  boot info cap start: ");
        puthex32(bi->untyped.start);
        puts("\n");
        fail("cap start mismatch");
    }

    if (untyped_info.cap_end != bi->untyped.end) {
        puts("MON|ERROR: cap end mismatch. Expected cap end: ");
        puthex32(untyped_info.cap_end);
        puts("  boot info cap end: ");
        puthex32(bi->untyped.end);
        puts("\n");
        fail("cap end mismatch");
    }

    for (unsigned i = 0; i < untyped_info.cap_end - untyped_info.cap_start; i++) {
        if (untyped_info.regions[i].paddr != bi->untypedList[i].paddr) {
            puts("MON|ERROR: paddr mismatch for untyped region: ");
            puthex32(i);
            puts("  expected paddr: ");
            puthex64(untyped_info.regions[i].paddr);
            puts("  boot info paddr: ");
            puthex64(bi->untypedList[i].paddr);
            puts("\n");
            fail("paddr mismatch");
        }
        if (untyped_info.regions[i].size_bits != bi->untypedList[i].sizeBits) {
            puts("MON|ERROR: size_bits mismatch for untyped region: ");
            puthex32(i);
            puts("  expected size_bits: ");
            puthex32(untyped_info.regions[i].size_bits);
            puts("  boot info size_bits: ");
            puthex32(bi->untypedList[i].sizeBits);
            puts("\n");
            fail("size_bits mismatch");
        }
        if (untyped_info.regions[i].is_device != bi->untypedList[i].isDevice) {
            puts("MON|ERROR: is_device mismatch for untyped region: ");
            puthex32(i);
            puts("  expected is_device: ");
            puthex32(untyped_info.regions[i].is_device);
            puts("  boot info is_device: ");
            puthex32(bi->untypedList[i].isDevice);
            puts("\n");
            fail("is_device mismatch");
        }
    }

    puts("MON|INFO: bootinfo untyped list matches expected list\n");
}

static unsigned
perform_invocation(seL4_Word *invocation_data, unsigned offset, unsigned idx)
{
    seL4_MessageInfo_t tag, out_tag;
    seL4_Error result;
    seL4_Word mr0;
    seL4_Word mr1;
    seL4_Word mr2;
    seL4_Word mr3;
    seL4_Word service;
    seL4_Word service_incr;
    seL4_Word cmd = invocation_data[offset];
    seL4_Word iterations = (cmd >> 32) + 1;
    seL4_Word tag0 = cmd & 0xffffffffULL;
    unsigned int cap_offset, cap_incr_offset, cap_count;
    unsigned int mr_offset, mr_incr_offset, mr_count;
    unsigned int next_offset;

    tag.words[0] = tag0;
    service = invocation_data[offset + 1];
    cap_count = seL4_MessageInfo_get_extraCaps(tag);
    mr_count = seL4_MessageInfo_get_length(tag);

#if 0
        puts("Doing invocation: ");
        puthex32(idx);
        puts(" cap count: ");
        puthex32(cap_count);
        puts(" MR count: ");
        puthex32(mr_count);
        puts("\n");
#endif

    cap_offset = offset + 2;
    mr_offset = cap_offset + cap_count;
    if (iterations > 1) {
        service_incr = invocation_data[mr_offset + mr_count];
        cap_incr_offset = mr_offset + mr_count + 1;
        mr_incr_offset = cap_incr_offset + cap_count;
        next_offset = mr_incr_offset + mr_count;
    } else {
        next_offset = mr_offset + mr_count;
    }

    if (seL4_MessageInfo_get_capsUnwrapped(tag) != 0) {
        fail("kernel invocation should never have unwrapped caps");
    }

    for (unsigned i = 0; i < iterations; i++) {
#if 0
        puts("Preparing invocation:\n");
#endif
        /* Set all the caps */
        seL4_Word call_service = service;
        if (i > 0) {
            call_service += service_incr * i;
        }
        for (unsigned j = 0; j < cap_count; j++) {
            seL4_Word cap = invocation_data[cap_offset + j];
            if (i > 0) {
                cap += invocation_data[cap_incr_offset + j] * i;
            }
#if 0
            puts("   SetCap: ");
            puthex32(j);
            puts(" ");
            puthex64(cap);
            puts("\n");
#endif
            seL4_SetCap(j, cap);
        }

        for (unsigned j = 0; j < mr_count; j++) {
            seL4_Word mr = invocation_data[mr_offset + j];
            if (i > 0) {
                mr += invocation_data[mr_incr_offset + j] * i;
            }
#if 0
            puts("   SetMR: ");
            puthex32(j);
            puts(" ");
            puthex64(mr);
            puts("\n");
#endif
            switch (j) {
                case 0: mr0 = mr; break;
                case 1: mr1 = mr; break;
                case 2: mr2 = mr; break;
                case 3: mr3 = mr; break;
                default: seL4_SetMR(j, mr); break;
            }
        }

        out_tag = seL4_CallWithMRs(call_service, tag, &mr0, &mr1, &mr2, &mr3);
        result = (seL4_Error) seL4_MessageInfo_get_label(out_tag);
        if (result != seL4_NoError) {
            puts("ERROR: ");
            puthex64(result);
            puts(" ");
            puts(sel4_strerror(result));
            puts("  invocation idx: ");
            puthex32(idx);
            puts(".");
            puthex32(i);
            puts("\n");
            fail("invocation error");
        }
#if 0
        puts("Done invocation: ");
        puthex32(idx);
        puts(".");
        puthex32(i);
        puts("\n");
#endif
    }
    return next_offset;
}

static void
print_registers(seL4_UserContext regs)
{
#if defined(ARCH_riscv64)
    puts("Registers: \n");
    puts("pc : ");
    puthex64(regs.pc);
    puts("\n");
    puts("ra : ");
    puthex64(regs.ra);
    puts("\n");
    puts("s0 : ");
    puthex64(regs.s0);
    puts("\n");
    puts("s1 : ");
    puthex64(regs.s1);
    puts("\n");
    puts("s2 : ");
    puthex64(regs.s2);
    puts("\n");
    puts("s3 : ");
    puthex64(regs.s3);
    puts("\n");
    puts("s4 : ");
    puthex64(regs.s4);
    puts("\n");
    puts("s5 : ");
    puthex64(regs.s5);
    puts("\n");
    puts("s6 : ");
    puthex64(regs.s6);
    puts("\n");
    puts("s7 : ");
    puthex64(regs.s7);
    puts("\n");
    puts("s8 : ");
    puthex64(regs.s8);
    puts("\n");
    puts("s9 : ");
    puthex64(regs.s9);
    puts("\n");
    puts("s10 : ");
    puthex64(regs.s10);
    puts("\n");
    puts("s11 : ");
    puthex64(regs.s11);
    puts("\n");
    puts("a0 : ");
    puthex64(regs.a0);
    puts("\n");
    puts("a1 : ");
    puthex64(regs.a1);
    puts("\n");
    puts("a2 : ");
    puthex64(regs.a2);
    puts("\n");
    puts("a3 : ");
    puthex64(regs.a3);
    puts("\n");
    puts("a4 : ");
    puthex64(regs.a4);
    puts("\n");
    puts("a5 : ");
    puthex64(regs.a5);
    puts("\n");
    puts("a6 : ");
    puthex64(regs.a6);
    puts("\n");
    puts("t0 : ");
    puthex64(regs.t0);
    puts("\n");
    puts("t1 : ");
    puthex64(regs.t1);
    puts("\n");
    puts("t2 : ");
    puthex64(regs.t2);
    puts("\n");
    puts("t3 : ");
    puthex64(regs.t3);
    puts("\n");
    puts("t4 : ");
    puthex64(regs.t4);
    puts("\n");
    puts("t5 : ");
    puthex64(regs.t5);
    puts("\n");
    puts("t6 : ");
    puthex64(regs.t6);
    puts("\n");
    puts("tp : ");
    puthex64(regs.tp);
#elif defined(ARCH_aarch64)
    // FIXME: Would be good to print the whole register set
    puts("Registers: \n");
    puts("pc : ");
    puthex64(regs.pc);
    puts("\n");
    puts("spsr : ");
    puthex64(regs.spsr);
    puts("\n");
    puts("x0 : ");
    puthex64(regs.x0);
    puts("\n");
    puts("x1 : ");
    puthex64(regs.x1);
    puts("\n");
    puts("x2 : ");
    puthex64(regs.x2);
    puts("\n");
    puts("x3 : ");
    puthex64(regs.x3);
    puts("\n");
    puts("x4 : ");
    puthex64(regs.x4);
    puts("\n");
    puts("x5 : ");
    puthex64(regs.x5);
    puts("\n");
    puts("x6 : ");
    puthex64(regs.x6);
    puts("\n");
    puts("x7 : ");
    puthex64(regs.x7);
    puts("\n");
#endif
}

static void
monitor(void)
{
    for (;;) {
        seL4_Word badge, label;
        seL4_MessageInfo_t tag;
        seL4_Error err;

        tag = seL4_Recv(fault_ep, &badge, reply);
        label = seL4_MessageInfo_get_label(tag);

        seL4_Word tcb_cap = tcbs[badge];

        puts("MON|ERROR: received message ");
        puthex32(label);
        puts("  badge: ");
        puthex64(badge);
        puts("  tcb cap: ");
        puthex64(tcb_cap);
        puts("\n");

        if (label == seL4_Fault_NullFault && badge < MAX_PDS) {
            /* This is a request from our PD to become passive */ 
            err = seL4_SchedContext_UnbindObject(scheduling_contexts[badge], tcb_cap);
            err = seL4_SchedContext_Bind(scheduling_contexts[badge], notification_caps[badge]);
            if (err != seL4_NoError) {
                puts("error binding scheduling context to notification");
            } else {
                puts(pd_names[badge]);
                puts(" is now passive!\n");
            }
            continue;
        }

        if (badge < MAX_PDS && pd_names[badge][0] != 0) {
            puts("MON|ERROR: faulting PD: ");
            puts(pd_names[badge]);
            puts("\n");
        } else {
            fail("unknown/invalid badge\n");
        }

        seL4_UserContext regs;

        err = seL4_TCB_ReadRegisters(tcb_cap, false, 0, sizeof(seL4_UserContext) / sizeof(seL4_Word), &regs);
        if (err != seL4_NoError) {
            fail("error reading registers");
        }

        print_registers(regs);

        switch(label) {
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
                    case seL4_NoFailure: puts("seL4_NoFailure"); break;
                    case seL4_InvalidRoot: puts("seL4_InvalidRoot"); break;
                    case seL4_MissingCapability: puts("seL4_MissingCapability"); break;
                    case seL4_DepthMismatch: puts("seL4_DepthMismatch"); break;
                    case seL4_GuardMismatch: puts("seL4_GuardMismatch"); break;
                    default: puthex64(lookup_failure_type);
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
                puts("MON|ERROR:    ec: ");
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
                    puts("MON|ERROR:    dfsc = ");
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

                break;
            }
            default:
                puts("Unknown fault\n");
                break;
        }
    }
}

void
main(seL4_BootInfo *bi)
{
    __sel4_ipc_buffer = bi->ipcBuffer;
    puts("MON|INFO: seL4 Core Platform Bootstrap\n");

#if 0
    /* This can be useful to enable during new platform bring up
     * if there are problems
     */
    dump_bootinfo(bi);
#endif

    check_untypeds_match(bi);

    puts("MON|INFO: Number of bootstrap invocations: ");
    puthex32(bootstrap_invocation_count);
    puts("\n");

    puts("MON|INFO: Number of system invocations:    ");
    puthex32(system_invocation_count);
    puts("\n");

    unsigned offset = 0;
    for (unsigned idx = 0; idx < bootstrap_invocation_count; idx++) {
        offset = perform_invocation(bootstrap_invocation_data, offset, idx);
    }
    puts("MON|INFO: completed bootstrap invocations\n");

    offset = 0;
    for (unsigned idx = 0; idx < system_invocation_count; idx++) {
        offset = perform_invocation(system_invocation_data, offset, idx);
    }

    puts("MON|INFO: completed system invocations\n");

    monitor();
}
