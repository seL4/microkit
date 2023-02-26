/*
 * Copyright 2022, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <sel4cp.h>

#define SERVER_CH 0 

void
init(void)
{
    sel4cp_dbg_puts("client: client protection domain init function running\n");

    /* message the server */
    sel4cp_mr_set(0, 0);
    (void) sel4cp_ppcall(SERVER_CH, sel4cp_msginfo_new(1, 1));
}

void
notified(sel4cp_channel ch)
{
    sel4cp_dbg_puts("Client recieved a notification on an unexpected channel\n");
}
