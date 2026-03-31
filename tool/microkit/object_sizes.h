/*
 * Copyright 2026, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <sel4/constants.h>
#include <sel4/arch/constants.h>
#include <sel4/sel4_arch/constants.h>

/*
 * The Microkit tool needs to know the sizes of kernel objects.
 * seL4 exports these values through C header files, to make it
 * easier for the tool to consume the object sizes, we preprocess
 * this file to extract the sizes of objects and then build_sdk.py
 * takes that output and mangles it into JSON.
 */

tcb: seL4_TCBBits
endpoint: seL4_EndpointBits
notification: seL4_NotificationBits
small_page: seL4_PageBits
large_page: seL4_LargePageBits
asid_pool: seL4_ASIDPoolBits
asid_table: seL4_ASIDPoolIndexBits
slot: seL4_SlotBits
min_untyped_bits: seL4_MinUntypedBits
max_untyped_bits: seL4_MaxUntypedBits
vspace: seL4_VSpaceBits

#ifdef seL4_ReplyBits
reply: seL4_ReplyBits
#endif

#ifdef seL4_VCPUBits
vcpu: seL4_VCPUBits
#endif
#ifdef seL4_PageTableBits
page_table: seL4_PageTableBits
#endif
#ifdef seL4_HugePageBits
huge_page: seL4_HugePageBits
#endif
#ifdef seL4_IOPageTableBits
io_page_table: seL4_IOPageTableBits
#endif
