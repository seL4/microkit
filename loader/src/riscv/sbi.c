/*
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include "sbi.h"

struct sbi_ret sbi_call(uint64_t eid, uint64_t fid, uint64_t arg0, uint64_t arg1, uint64_t arg2, uint64_t arg3,
                        uint64_t arg4, uint64_t arg5)
{
    struct sbi_ret ret;

    register uint64_t a0 asm("a0") = arg0;
    register uint64_t a1 asm("a1") = arg1;
    register uint64_t a2 asm("a2") = arg2;
    register uint64_t a3 asm("a3") = arg3;
    register uint64_t a4 asm("a4") = arg4;
    register uint64_t a5 asm("a5") = arg5;
    register uint64_t a6 asm("a6") = fid;
    register uint64_t a7 asm("a7") = eid;

    asm volatile("ecall"
                 : "+r"(a0), "+r"(a1)
                 : "r"(a0), "r"(a1), "r"(a2), "r"(a3), "r"(a4), "r"(a5), "r"(a6), "r"(a7)
                 : "memory");

    ret.error = a0;
    ret.value = a1;

    return ret;
}

/*
 * Chapter 3, Table 1 of SBI specification.
 */
char *sbi_error_as_string(long error)
{
    switch (error) {
    case SBI_SUCCESS:
        return "Completed successfully";
    case SBI_ERR_FAILED:
        return "Failed";
    case SBI_ERR_NOT_SUPPORTED:
        return "Not supported";
    case SBI_ERR_INVALID_PARAM:
        return "Invalid parameter(s)";
    case SBI_ERR_DENIED:
        return "Denied or not allowed";
    case SBI_ERR_INVALID_ADDRESS:
        return "Invalid address(s)";
    case SBI_ERR_ALREADY_AVAILABLE:
        return "Already available";
    case SBI_ERR_ALREADY_STARTED:
        return "Already started";
    case SBI_ERR_ALREADY_STOPPED:
        return "Already stopped";
    case SBI_ERR_NO_SHMEM:
        return "Shared memory not available";
    case SBI_ERR_INVALID_STATE:
        return "Invalid state";
    case SBI_ERR_BAD_RANGE:
        return "Bad (or invalid) range";
    case SBI_ERR_TIMEOUT:
        return "Failed due to timeout";
    case SBI_ERR_IO:
        return "Input/Output error";
    case SBI_ERR_DENIED_LOCKED:
        return "Denied or not allowed due to lock status";
    default:
        return "<unknown error>";
    }
}
