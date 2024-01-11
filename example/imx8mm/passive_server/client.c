/*
 * Copyright 2022, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

#include <microkit.h>

#define SERVER_CH 0

void init(void)
{
    microkit_dbg_puts("CLIENT|INFO: init function running\n");

    /* message the server */
    (void) microkit_ppcall(SERVER_CH, microkit_msginfo_new(1, 1));
}

void notified(microkit_channel ch)
{
    microkit_dbg_puts("CLIENT|INFO: recieved a notification on an unexpected channel\n");
}
