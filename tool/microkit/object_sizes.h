/*
 * Copyright 2026, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <sel4/arch/constants.h>
#include <sel4/constants.h>
#include <sel4/plat/api/constants.h>
#include <sel4/sel4_arch/constants.h>

/*
 * The Microkit tool needs to know the sizes of kernel objects and a small
 * amount of address translation information.
 * seL4 exports these values through C header files, to make it
 * easier for the tool to consume the object sizes, we preprocess
 * this file to extract the sizes of objects and then build_sdk.py
 * takes that output and mangles it into JSON.
 */

#define MICROKIT_CONSTANT(name, value) microkit_constant name value

MICROKIT_CONSTANT(tcb, seL4_TCBBits)
MICROKIT_CONSTANT(endpoint, seL4_EndpointBits)
MICROKIT_CONSTANT(notification, seL4_NotificationBits)
MICROKIT_CONSTANT(small_page, seL4_PageBits)
MICROKIT_CONSTANT(large_page, seL4_LargePageBits)
MICROKIT_CONSTANT(asid_pool, seL4_ASIDPoolBits)
MICROKIT_CONSTANT(asid_table, seL4_ASIDPoolIndexBits)
MICROKIT_CONSTANT(slot, seL4_SlotBits)
MICROKIT_CONSTANT(min_untyped_bits, seL4_MinUntypedBits)
MICROKIT_CONSTANT(max_untyped_bits, seL4_MaxUntypedBits)
MICROKIT_CONSTANT(vspace, seL4_VSpaceBits)
MICROKIT_CONSTANT(page_table_index_bits, seL4_PageTableIndexBits)
#ifdef seL4_UserVSpaceTop
MICROKIT_CONSTANT(user_top, seL4_UserVSpaceTop)
#else
//! TODO: Remove once microkit points to the version of seL4 where seL4_UserVSpaceTop is always defined.
MICROKIT_CONSTANT(user_top, seL4_UserTop)
#endif

#ifdef seL4_VSpaceIndexBits
MICROKIT_CONSTANT(vspace_index_bits, seL4_VSpaceIndexBits)
#endif

#ifdef seL4_ReplyBits
MICROKIT_CONSTANT(reply, seL4_ReplyBits)
#endif

#ifdef seL4_VCPUBits
MICROKIT_CONSTANT(vcpu, seL4_VCPUBits)
#endif
#ifdef seL4_PageTableBits
MICROKIT_CONSTANT(page_table, seL4_PageTableBits)
#endif
#ifdef seL4_HugePageBits
MICROKIT_CONSTANT(huge_page, seL4_HugePageBits)
#endif
#ifdef seL4_IOPageTableBits
MICROKIT_CONSTANT(io_page_table, seL4_IOPageTableBits)
#endif

#ifdef VTD_PT_INDEX_BITS
MICROKIT_CONSTANT(io_page_table_index_bits, VTD_PT_INDEX_BITS)
#endif
