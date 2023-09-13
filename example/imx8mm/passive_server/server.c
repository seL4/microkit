/*
 * Copyright 2022, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <microkit.h>

microkit_msginfo protected(microkit_channel ch, microkit_msginfo msginfo)
{
    switch (microkit_msginfo_get_label(msginfo)) {
    case 0:
        microkit_dbg_puts("server: is running on clients scheduling context\n");
        break;
    default:
        microkit_dbg_puts("server: received an unexpected message\n");
    }

    return seL4_MessageInfo_new(0, 0, 0, 0);
}

void init(void)
{
    microkit_dbg_puts("server: server protection domain init function running\n");
    /* Nothing to initialise */
}

void notified(microkit_channel ch)
{
    microkit_dbg_puts("server: recieved a notification on an unexpected channel\n");
}
