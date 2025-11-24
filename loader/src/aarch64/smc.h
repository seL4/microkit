/*
 * Copyright 2025, UNSW.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#pragma once

#include <stdint.h>

#include "../cutil.h"
#include "../uart.h"

/**
 * The Power State Coordinate Interface (DEN0022F.b) document in §5.2.1
 * specifies that
 *
 *      For PSCI functions that use only 32-bit parameters, the arguments are
 *      passed in R0 to R3 (AArch32) or W0 to W3 (AArch64), with return values
 *      in R0 or W0. For versions using 64-bit parameters, the arguments are
 *      passed in X0 to X3, with return values in X0. In line with the SMC
 *      Calling Conventions, the immediate value used with an SMC (or HVC)
 *      instruction must be 0.
 *
 * Hence we need both the SMC64 and SMC32 conventions implemented, with support
 * for up to 3 arguments.
 *
 * Table 5 Return error codes gives the error codes.
 * Errors are 32-bit signed integers for SMC32 functions, and 64-bit signed integers for SMC64 functions
 **/

#define PSCI_RETURN_SUCCESS 0
#define PSCI_RETURN_NOT_SUPPORTED -1
#define PSCI_RETURN_INVALID_PARAMETERS -2
#define PSCI_RETURN_DENIED -3
#define PSCI_RETURN_ALREADY_ON -4
#define PSCI_RETURN_ON_PENDING -5
#define PSCI_RETURN_INTERNAL_FAILURE -6
#define PSCI_RETURN_NOT_PRESENT -7
#define PSCI_RETURN_DISABLED -8
#define PSCI_RETURN_INVALID_ADDRESS -9

/* this is fine for both 64-bit and 32-bit return codes as a 0xFFFFFFFF'XXXXXXXX code
   will get truncated to the 0xXXXXXXXX which is still -1 as 32-bit */
static inline const char *psci_return_as_string(uint32_t ret)
{
    switch (ret) {
    case PSCI_RETURN_SUCCESS:
        return "SUCCESS";
    case PSCI_RETURN_NOT_SUPPORTED:
        return "NOT_SUPPORTED";
    case PSCI_RETURN_INVALID_PARAMETERS:
        return "INVALID_PARAMETERS";
    case PSCI_RETURN_DENIED:
        return "DENIED";
    case PSCI_RETURN_ALREADY_ON:
        return "ALREADY_ON";
    case PSCI_RETURN_ON_PENDING:
        return "ON_PENDING";
    case PSCI_RETURN_INTERNAL_FAILURE:
        return "INTERNAL_FAILURE";
    case PSCI_RETURN_NOT_PRESENT:
        return "NOT_PRESENT";
    case PSCI_RETURN_DISABLED:
        return "DISABLED";
    case PSCI_RETURN_INVALID_ADDRESS:
        return "INVALID_ADDRESS";
    default:
        return "<unknown return>";
    }
}

/* §5.1.4 of PSCI */
#define PSCI_FUNCTION_VERSION 0x84000000
#define PSCI_FUNCTION_CPU_ON 0xC4000003

/**
 * See document DEN002E SMC Calling Convention (v1.4, May 2022),
 * specifically §2.7 "SMC64/HVC64 argument passing".
 *
 * We only support up to 4 arguments, but the actual convention supports up
 * to 17, and clobbers X4--X17. The convention also supports up to 17 returns,
 * but we again only support 1.
 *
 **/
static inline uint64_t arm_smc64_call(uint32_t function_id, uint64_t arg0, uint64_t arg1, uint64_t arg2)
{
    // Per Table 2-1, BIT(30)==1 defines the SMC64 calling convention
    if ((function_id & BIT(30)) == 0) {
        puts("LDR|ERROR: SMC32 function used in SMC64 call\n");
        return PSCI_RETURN_INVALID_PARAMETERS;
    }

    register uint64_t x0 asm("x0") = function_id;
    register uint64_t x1 asm("x1") = arg0;
    register uint64_t x2 asm("x2") = arg1;
    register uint64_t x3 asm("x3") = arg2;
    asm volatile(
        "smc #0\n"
        : "=r"(x0)
        : "r"(x0), "r"(x1), "r"(x2), "r"(x3)
        : "x4", "x5", "x6", "x7", "x8", "x9", "x10", "x11", "x12", "x13", "x14", "x15", "x16", "x17",
        "memory"
    );

    return x0;
}

/**
 * Reference 2.6 "SMC32/HVC32 argument passing".
 **/
static inline uint32_t arm_smc32_call(uint32_t function_id, uint32_t arg0, uint32_t arg1, uint32_t arg2)
{
    // Per Table 2-1, BIT(30)==0 defines the SMC32 calling convention
    if ((function_id & BIT(30)) == BIT(30)) {
        puts("LDR|ERROR: SMC64 function used in SMC32 call\n");
        return PSCI_RETURN_INVALID_PARAMETERS;
    }
    // TODO: This only supports AArch64 mode for the moment, in AArch32 mode
    //       we need to use the x... registers.

    register uint64_t w0 asm("w0") = function_id;
    register uint64_t w1 asm("w1") = arg0;
    register uint64_t w2 asm("w2") = arg1;
    register uint64_t w3 asm("w3") = arg2;
    asm volatile(
        "smc #0\n"
        : "=r"(w0)
        : "r"(w0), "r"(w1), "r"(w2), "r"(w3)
        : "w4", "w5", "w6", "w7", "w8", "w9", "w10", "w11", "w12", "w13", "w14", "w15", "w16", "w17",
        "memory"
    );

    return w0;
}
