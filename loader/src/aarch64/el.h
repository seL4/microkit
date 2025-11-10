/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 * Copyright 2025, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#pragma once

#include <stdint.h>

enum el {
    EL0 = 0,
    EL1 = 1,
    EL2 = 2,
    EL3 = 3,
};

enum el current_el(void);
int ensure_correct_el(void);

static inline const char *el_to_string(enum el el)
{
    switch (el) {
    case EL0:
        return "EL0";
    case EL1:
        return "EL1";
    case EL2:
        return "EL2";
    case EL3:
        return "EL3";
    }

    return "<invalid el>";
}
