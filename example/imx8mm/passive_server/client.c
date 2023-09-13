/*
 * Copyright 2022, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <microkit.h>

#define SERVER_CH 0 

void
init(void)
{
    microkit_dbg_puts("client: client protection domain init function running\n");

    /* message the server */
    microkit_mr_set(0, 0);
    (void) microkit_ppcall(SERVER_CH, microkit_msginfo_new(1, 1));
}

void
notified(microkit_channel ch)
{
    microkit_dbg_puts("client: recieved a notification on an unexpected channel\n");
}
