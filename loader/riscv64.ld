/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
PHDRS
{
    all PT_LOAD AT (LINK_ADDRESS);
}

SECTIONS
{
    . = LINK_ADDRESS;

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
        __global_pointer$ = . + 0x800;
    *(.srodata)
    *(.sdata)
       _data_end = .;
    } :all

    .bss :
    {
        _bss = .;
    *(.sbss)
        *(.bss)
        *(COMMON)
        . = ALIGN(4);
        _bss_end = .;
    } :all
}
