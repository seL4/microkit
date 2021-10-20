/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
PHDRS
{
/*    all PT_LOAD AT (0x51fa0000) ; */
/*     all PT_LOAD AT (0x80000) ; */
    all PT_LOAD AT (0x80280000);
}

SECTIONS
{
/*   . = 0x51fa0000; */
/*    . = 0x80000; */
    . = 0x80280000;

    .text :
    {
        _text = .;
        *(.text.start)
        *(.text*)
        *(.rodata)
        _text_end = .;
    } :all

    .data :
    {
        _data = .;
        *(.data)
        _data_end = .;
    } :all

    .bss :
    {
        _bss = .;
        *(.bss)
        *(COMMON)
        . = ALIGN(4);
        _bss_end = .;
    } :all
}
