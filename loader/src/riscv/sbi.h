/*
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#pragma once

#include <stddef.h>
#include <stdint.h>

#define SBI_EXT_BASE 0x10
#define SBI_EXT_HSM 0x48534D
#define SBI_EXT_DEBUG_CONSOLE 0x4442434E

#define SBI_HSM_HART_START 0x0
#define SBI_HSM_HART_STOP 0x1

#define SBI_DEBUG_CONSOLE_WRITE_BYTE 0x2

struct sbi_ret {
    uint64_t error;
    uint64_t value;
};

enum sbi_error {
    SBI_SUCCESS = 0,
    SBI_ERR_FAILED = -1,
    SBI_ERR_NOT_SUPPORTED = -2,
    SBI_ERR_INVALID_PARAM = -3,
    SBI_ERR_DENIED = -4,
    SBI_ERR_INVALID_ADDRESS = -5,
    SBI_ERR_ALREADY_AVAILABLE = -6,
    SBI_ERR_ALREADY_STARTED = -7,
    SBI_ERR_ALREADY_STOPPED = -8,
    SBI_ERR_NO_SHMEM = -9,
    SBI_ERR_INVALID_STATE = -10,
    SBI_ERR_BAD_RANGE = -11,
    SBI_ERR_TIMEOUT = -12,
    SBI_ERR_IO = -13,
    SBI_ERR_DENIED_LOCKED = -14,
};

struct sbi_ret sbi_call(uint64_t eid, uint64_t fid, uint64_t arg0, uint64_t arg1, uint64_t arg2, uint64_t arg3,
                        uint64_t arg4, uint64_t arg5);
char *sbi_error_as_string(long error);
