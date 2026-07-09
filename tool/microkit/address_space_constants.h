/*
 * Copyright 2026, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <sel4/arch/constants.h>
#include <sel4/constants.h>
#include <sel4/plat/api/constants.h>
#include <sel4/sel4_arch/constants.h>

page_table_index_bits: seL4_PageTableIndexBits
vspace_user_top: seL4_UserVSpaceTop

#ifdef seL4_VSpaceIndexBits
vspace_index_bits: seL4_VSpaceIndexBits
#endif

#ifdef seL4_IOPageTableIndexBits
io_page_table_index_bits: seL4_IOPageTableIndexBits
#endif
