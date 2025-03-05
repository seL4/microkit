/*
 * Copyright 2017, Data61, CSIRO (ABN 41 687 119 230)
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

/*
 * This domain schedule is intended to be filled in via ELF patching
 */

#include <config.h>
#include <object/structures.h>
#include <model/statedata.h>

dschedule_t ksDomSchedule[256];
word_t ksDomScheduleLength;
