/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
PHDRS
{
    text PT_LOAD;
    data PT_LOAD;
}

ENTRY(_start);

STARTUP(crt0.o);

SECTIONS
{
    . = 0x200000;

    .text :
    {
        _text = .;
        *(.text.start)
        *(.text*)
        *(.rodata)
        _text_end = .;
    } :text

    . = ALIGN(0x1000);

    /* For some reason the ABI puts init array
     * into writable memory, so we have to follow suit */
    .init_array :
    {
        PROVIDE(__init_array_start = .);
        KEEP (*(SORT(.init_array.*)))
        KEEP (*(.init_array*))
        PROVIDE(__init_array_end = .);
    } :data

    .data :
    {
        _data = .;
        *(.data)
        _data_end = .;
    } :data

    .bss :
    {
        _bss = .;
        *(.bss)
        *(COMMON)
        . = ALIGN(4);
        _bss_end = .;
    } :data

    . = ALIGN(0x1000);
    .ipc_buffer (NOLOAD): {
        __sel4_ipc_buffer_obj = .;
    }
}
