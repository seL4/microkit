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

        KEEP(*(.text.start))
        *(.text .text.*)

        _text_end = .;
    } :all

    .rodata :
    {
        *(.rodata .rodata.* .rodata..Lanon.*)
    } :all

    .data :
    {
        _data = .;
        *(.data .data.*)
        *(.data.*)
        __global_pointer$ = . + 0x800;
        *(.srodata .srodata.*)
        *(.sdata .sdata.*)

        KEEP(*(.data.uart_addr))

        _data_end = .;
    } :all

    .bss :
    {
        _bss = .;
        *(.sbss .sbss.*)
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(4);
        _bss_end = .;
    } :all
}
