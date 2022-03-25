/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <sel4cp.h>

static uint8_t restart_count = 0;

static char
decchar(unsigned int v) {
    return '0' + v;
}

static void
put8(uint8_t x)
{
    char tmp[4];
    unsigned i = 3;
    tmp[3] = 0;
    do {
        uint8_t c = x % 10;
        tmp[--i] = decchar(c);
        x /= 10;
    } while (x);
    sel4cp_dbg_puts(&tmp[i]);
}

void
init(void)
{
    sel4cp_dbg_puts("restarter: starting\n");
}

void
notified(sel4cp_channel ch)
{
}

seL4_MessageInfo_t
protected(sel4cp_channel ch, sel4cp_msginfo msginfo)
{
    sel4cp_dbg_puts("restarter: received protected message\n");

    return sel4cp_msginfo_new(0, 0);
}

void
fault(sel4cp_pd pd, sel4cp_msginfo msginfo)
{
    sel4cp_dbg_puts("restarter: received fault message for pd: ");
    put8(pd);
    sel4cp_dbg_puts("\n");
    restart_count++;
    if (restart_count < 10) {
        sel4cp_pd_restart(pd, 0x200000);
        sel4cp_dbg_puts("restarter: restarted\n");
    } else {
        sel4cp_pd_stop(pd);
        sel4cp_dbg_puts("restarter: too many restarts - PD stopped\n");
    }
}
