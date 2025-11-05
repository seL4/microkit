/*
 * Copyright 2026, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

#define CH_SECONDARY ((microkit_channel)0)

// As per cap_sharing.system
#define CAP_SECONDARY_SC  (microkit_cspace_slot_to_cptr(1))
#define CAP_SECONDARY_TCB (microkit_cspace_slot_to_cptr(2))
#define CAP_MY_SC         (microkit_cspace_slot_to_cptr(3))
#define CAP_MY_TCB        (microkit_cspace_slot_to_cptr(4))

static void halt(void)
{
    seL4_Error error = seL4_TCB_Suspend(CAP_MY_TCB);
    if (error != seL4_NoError) {
        microkit_dbg_puts("|primary  | error suspending TCB\n");
    }

    microkit_dbg_puts("|primary  | error: should not reach this point! we should have suspended ourself!\n");
    while (1) { }
}

void init(void)
{
    seL4_Error err;

    microkit_dbg_puts("|primary  | hello, world\n");

    /* Notify the secondary. This will print output from secondary as it is
       higher priority. */
    microkit_dbg_puts("|primary  | notifying secondary\n");
    microkit_notify(CH_SECONDARY);

    microkit_dbg_puts("|primary  | suspending secondary\n");
    err = seL4_TCB_Suspend(CAP_SECONDARY_TCB);
    if (err != seL4_NoError) {
        microkit_dbg_puts("|primary  | error suspending TCB\n");
        halt();
    }

    /* Notify the secondary. It is suspended so it will not print. */
    microkit_dbg_puts("|primary  | notifying secondary (it should not print)\n");
    microkit_notify(CH_SECONDARY);

    microkit_dbg_puts("|primary  | resuming secondary (it should then print)\n");
    err = seL4_TCB_Resume(CAP_SECONDARY_TCB);
    if (err != seL4_NoError) {
        microkit_dbg_puts("|primary  | error resuming TCB\n");
        halt();
    }

    microkit_dbg_puts("|primary  | halting (success)...\n");
    halt();
}

void notified(microkit_channel ch)
{
}
