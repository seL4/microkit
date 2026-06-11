/*
 * Copyright 2026, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
#include <stdint.h>
#include <microkit.h>

#define BEGIN_MAGIC 0x53145314
#define VALUE_A     0x01234567
#define VALUE_B     0x89ABCDEF
#define END_MAGIC   0x75757575

typedef struct __attribute__((packed)) fill_data {
    uint32_t begin_magic;
    uint32_t value_a;
    uint8_t _padding1[0x400];
    uint32_t value_b;
    uint8_t _padding2[0xc00];
    uint32_t end_magic;
} fill_data_t;

fill_data_t *filled_mr;
uint64_t filled_mr_data_size;

void init(void)
{
    microkit_dbg_puts("hello, world. my name is ");
    microkit_dbg_puts(microkit_name);
    microkit_dbg_puts("\n");

    microkit_dbg_puts("checking prefilled memory region data length\n");
    if (filled_mr_data_size != sizeof(fill_data_t)) {
        microkit_dbg_puts("oh no prefilled data length doesn't match: ");
        microkit_dbg_put32(filled_mr_data_size);
        microkit_dbg_puts(" != ");
        microkit_dbg_put32(sizeof(fill_data_t));
        microkit_dbg_puts("\n");
        return;
    }

    microkit_dbg_puts("checking prefilled memory region data\n");

    if (filled_mr->begin_magic != BEGIN_MAGIC) {
        microkit_dbg_puts("oh no begin magic doesn't match: ");
        microkit_dbg_put32(filled_mr->begin_magic);
        microkit_dbg_puts(" != ");
        microkit_dbg_put32(BEGIN_MAGIC);
        microkit_dbg_puts("\n");
        return;
    }

    if (filled_mr->value_a != VALUE_A) {
        microkit_dbg_puts("oh no value A doesn't match: ");
        microkit_dbg_put32(filled_mr->value_a);
        microkit_dbg_puts(" != ");
        microkit_dbg_put32(VALUE_A);
        microkit_dbg_puts("\n");
        return;
    }

    if (filled_mr->value_b != VALUE_B) {
        microkit_dbg_puts("oh no value B doesn't match: ");
        microkit_dbg_put32(filled_mr->value_b);
        microkit_dbg_puts(" != ");
        microkit_dbg_put32(VALUE_B);
        microkit_dbg_puts("\n");
        return;
    }

    if (filled_mr->end_magic != END_MAGIC) {
        microkit_dbg_puts("oh no end magic doesn't match: ");
        microkit_dbg_put32(filled_mr->end_magic);
        microkit_dbg_puts(" != ");
        microkit_dbg_put32(END_MAGIC);
        microkit_dbg_puts("\n");
        return;
    }

    microkit_dbg_puts("prefilled data OK!\n");
}

void notified(microkit_channel ch)
{
}
