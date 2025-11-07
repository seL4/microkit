/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <stdint.h>

static inline const char *ex_to_string(uintptr_t ex)
{
    switch (ex) {
    case 0:
        return "Synchronous (Current Exception level with SP_EL0)";
    case 1:
        return "IRQ (Current Exception level with SP_EL0)";
    case 2:
        return "FIQ (Current Exception level with SP_EL0)";
    case 3:
        return "SError (Current Exception level with SP_EL0)";
    case 4:
        return "Synchronous (Current Exception level with SP_ELx)";
    case 5:
        return "IRQ (Current Exception level with SP_ELx)";
    case 6:
        return "FIQ (Current Exception level with SP_ELx)";
    case 7:
        return "SError (Current Exception level with SP_ELx)";
    case 8:
        return "Synchronous 64-bit EL0";
    case 9:
        return "IRQ 64-bit EL0";
    case 10:
        return "FIQ 64-bit EL0";
    case 11:
        return "SError 64-bit EL0";
    case 12:
        return "Synchronous 32-bit EL0";
    case 13:
        return "IRQ 32-bit EL0";
    case 14:
        return "FIQ 32-bit EL0";
    case 15:
        return "SError 32-bit EL0";
    }
    return "<invalid ex>";
}

static inline const char *ec_to_string(uintptr_t ec)
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
