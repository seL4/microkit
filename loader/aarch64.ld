/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
PHDRS
{
    all PT_LOAD AT (LINK_ADDRESS);
}


//    text PT_LOAD FLAGS(5); /* RX */
//    rodata PT_LOAD FLAGS(4); /* RO */
//    data PT_LOAD FLAGS(6); /* RW */
//    bss PT_LOAD FLAGS(6); /* RW */

SECTIONS
{
    . = LINK_ADDRESS;

    _loader_start = .;

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

        KEEP(*(.data.uart_addr))

        _data_end = .;
    } :all

    .bss :
    {
        _bss = .;
        *(.bss)
        *(.bss.*)
        *(COMMON)
        . = ALIGN(4);
        _bss_end = .;
    } :all

    _loader_end = .;
}
