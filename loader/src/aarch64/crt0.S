/*
 * Copyright 2021, Breakaway Consulting Pty. Ltd.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */
.extern main

.section ".text.start"

.global _start;
.type _start, %function;
_start:

     mrs    x0, mpidr_el1
     and    x0, x0,#0xFF        // Check processor id
     cbz    x0, master        // Hang for all non-primary CPU

proc_hang:
     wfe
     b proc_hang

master:
    adrp    x1, _stack
    add     x1, x1, #0xff0
    mov     sp, x1
    /* May not have been loaded in the right location.
     * Try and move ourselves so we're in the right place
     */
    bl     relocate
    cmp     x0, #1
    beq     1f
    /* Otherwise, jump to the start of the new loader */
    br      x0
1:
    b       main

relocate:
    stp     x29, x30, [sp, #-16]!
    mov     x29, sp
    /* Check if the image is already at the correct location */
    ldr     x0, =LINK_ADDRESS
    adr     x1, _start
    cmp     x0, x1
    beq     image_ok

    /* Log that we are being relocated */
    bl relocation_log

    /* Restore x0 and x1 */
    ldr     x0, =LINK_ADDRESS
    adr     x1, _start

    /* Sanity check: We don't want to overwrite ourselves! We assume that
     * everything between _start (src_start) and _bss_end (src_end) is important (i.e.
     * something that might be run while relocating) but allow overlap for
     * things after _bss_end i.e. the loader_data.
     */
    adrp    x2, _bss_end
    add     x2, x2, #:lo12:_bss_end

    /* The loader_data is directly after _bss_end, with the first
     * value being the loader_data struct. The second field of
     * this struct is the size of the loader_data region, so we
     * add this to _bss_end to get the real end of the image
     */
    ldr     x3, [x2, #+8]
    add     x2, x2, x3
    sub     x2, x2, x1

    adrp x3, _bss_end
    add x3, x3, #:lo12:_bss_end

    sub x5, x3, x1
    add x4, x0, x5 /* dst_end */

    /* At this point:
     *      x0: dst_code_start (LINK_ADDRESS)
     *      x1: src_code_start (_start)
     *      x2: image_size
     *      x3: src_code_end (_bss_end)
     *      x4: dst_code_end (LINK_ADDRESS + (_bss_end - _start))
     */

    /* check: if (dst_end >= src_start && dst_end < src_end) { abort } */
    cmp     x4, x1
    blt     1f

    cmp     x4, x3
    bge     1f

    b cant_reloc
1:

    /* check: if (dst_start >= src_start && dest_start < src_end) { abort } */
    cmp     x0, x1
    blt     2f

    cmp     x0, x3
    bge     2f
cant_reloc:
    b relocation_failed

2:
    /* x0 = desired image base */
    /* x1 = current image base */
    /* x2 = image size */
    bl memmove

    /* x0 = dest, save it to a callee-saved register while we invalidate icache */
    mov x19, x0
    bl flush_dcache
    bl invalidate_icache
    mov x0, x19
    b 1f

image_ok:
    /* Already in the right place, keep booting */
    mov x0, #1

1:
    ldp x29, x30, [sp], #16
    ret
