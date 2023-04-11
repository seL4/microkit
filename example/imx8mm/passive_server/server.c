
#include <sel4cp.h>

sel4cp_msginfo 
protected(sel4cp_channel ch, sel4cp_msginfo msginfo)
{
    switch (sel4cp_msginfo_get_label(msginfo)) {
        case 0:
            sel4cp_dbg_puts("server: is running on clients scheduling context\n");
            break;
        default:
            sel4cp_dbg_puts("server: received an unexpected message\n");
    }

    return seL4_MessageInfo_new(0, 0, 0, 0);
}

void
init(void)
{
    sel4cp_dbg_puts("server: server protection domain init function running\n");
    /* Nothing to initialise */
}

void
notified(sel4cp_channel ch)
{
    sel4cp_dbg_puts("server: recieved a notification on an unexpected channel\n");
}
