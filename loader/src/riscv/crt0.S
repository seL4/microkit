/*
 * Copyright 2024, UNSW
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

.extern main
.extern __global_pointer$

#include <kernel/gen_config.h>

#ifndef CONFIG_FIRST_HART_ID
#error "Loader must be defined with FIRST_HART_ID"
#endif

#include "../loader.h"
#include "sbi.h"

.section ".text.start"

#define STACK_SIZE 4096

/* Unfortunately, the latest version of the SBI (v1.0.0) specification does not
 * specify the starting state of supervisor software.
 * OpenSBI gives the following:
 *    a0: current hart ID
 *    a1: address of the DTB (given by previous booting stage)
 * However, these parameters are not enforced by the specification. Therefore, if you were
 * to use a different SBI implementation or your own, it might not be compatible with
 * this loader.
 * The hart ID is required as seL4 expects to be booted on CONFIG_FIRST_HART_ID.
 * The address of the DTB is passed to seL4 which is then passed to the initial
 * task (via the BootInfo frame).
 *
 * On RISC-V, only M-Mode can access the CSR mhartid to get the actual hart ID,
 * the SBI running there is responsible for passing this ID up. In S-Mode there
 * is no way to ever query it again, so we have to preserve what we get passed
 * here. This is a RISC-V design decision, more background can be found at
 * https://github.com/riscv/riscv-sbi-doc/issues/25.
 *
 * It seems that OpenSBI starts us at a random hart and keeps all other harts
 * suspended or spinning. However, even on non-SMP configurations there might
 * be an expectation that we are running on FIRST_HART_ID
 * hart turns out to be a different one, we have to switch harts somehow. The
 * SBI Heart State Management (HSM) extension exists for this, but it might not
 * be implemented. In this case, there is nothing we can do here in the assembly
 * startup code, but C boot code might still have platform specific proprietary
 * ways to switch harts.
 */

.global _start
_start:

.option push
.option norelax
1:auipc gp, %pcrel_hi(__global_pointer$)
  addi  gp, gp, %pcrel_lo(1b)
.option pop

  /* save the parameters passed */
  mv s0, a0 /* preserve a0 (hart id) in s0 */
  mv s2, a1 /* preserve a1 (dtb) in s2 */

  /* Attach the stack to sp before calling any C functions */
  la sp, (_stack + STACK_SIZE)

  /* Check if the Heart State Management (HSM) extension exists, so it can be
   * used to switch harts if we are not running on hart FIRST_HART_ID
   * The SBI returns SBI_SUCCESS (0) in a0 if the call could be processed or an
   * error code if not. On SBI_SUCCESS the value in a1 is 0 if the extension is
   * not available or an extension-specific non-zero value if it is available.
   */
  li a7, SBI_BASE_EID
  li a6, SBI_BASE_PROBE_EXT_FID
  li a0, SBI_BASE_EID
  ecall /* call SBI to probe for HSM extension */
  mv a2, a0 /* move SBI call generic return code to a2 as we need a0 */
  mv a3, a1 /* move SBI call error return code to a3 as we need a1 */
  mv a0, s0 /* restore a0 to hold hart ID passed by the boot loader */
  mv a1, s2 /* restore a1 to hold dtb address passed by the boot loader */
  bnez a2, _start1 /* goto _start1 if SBI did not return SBI_SUCCESS (0) */
  beqz a3, _start1 /* goto _start1 if HSM extension is missing */

  /* Check if we are running on the hart we expect on, FIRST_HART_ID */
  li s1, CONFIG_FIRST_HART_ID
  beq  a0, s1, _start1 /* goto _start1 if we are on FIRST_HART_ID */

  /* Use HSM extension to start hart FIRST_HART_ID. */
hsm_switch_hart:
  li a7, SBI_HSM_EID
  li a6, SBI_HSM_HART_START_FID
  li a0, CONFIG_FIRST_HART_ID
  mv a2, s2 /* dtb address to be passed in a1 when new hart starts is 3rd parameter */
  la a1, _start1 /* where to start the hart */
  ecall /* call SBI to start hart FIRST_HART_ID */

  /* Since we are not the designated primary hart, continue the boot process as
   * secondary hart
   */
  mv a0, s0 /* restore a0 to hold hart ID passed by OpenSBI */
  li a7, SBI_HSM_EID
  li a6, 0x1
  ecall

relocate:
  /* Save the return address */
  addi sp, sp, -0x8
  sd ra, (sp)

  /* Check if we are already in the right place */
  li a0, LINK_ADDRESS
  la a1, _start
  beq a0, a1, image_ok

  /* Log that we are being relocated */
  jal relocation_log

  /* Restore a0 and a1 */
  li a0, LINK_ADDRESS
  la a1, _start

  /* Sanity check: We don't want to overwrite ourselves! We assume that
   * everything between _start (src_start) and _bss_end (src_end) is important (i.e.
   * something that might be run while relocating) but allow overlap for
   * things after _bss_end i.e. the loader_data.
   */
  la a2, _bss_end

   /* The loader_data is directly after _bss_end, with the first
   * value being the loader_data struct. The second field of this
   * struct is the size of the loader_data region, so we add
   * this to _bss_end to get the real end of the image
   */
  ld a3, 8(a2)
  add a2, a2, a3
  sub a2, a2, a1

  la a3, _bss_end
  sub a5, a3, a1
  add a4, a0, a5

  /* At this point:
   *      a0: dst_start (LINK_ADDRESS)
   *      a1: src_start (_start)
   *      a2: image_size
   *      a3: src_end (_bss_end)
   *      a4: dst_end (LINK_ADDRESS + (_bss_end - _start))
   */

  /* check: if (dst_end >= src_start && dst_end < src_end) { abort } */
  blt a4, a1, 1f
  bge a4, a3, 1f
  j cant_reloc

1:

  /* check: if (dst_start >= src_start && dest_start < src_end) { abort } */
  blt a0, a1, 2f
  bge a0, a3, 2f

cant_reloc:
  j relocation_failed

2:
  /* a0 = desired image base */
  /* a1 = current image base */
  /* a2 = image size */
  jal memmove
  j 1f

image_ok:
  li a0, 1

1:
  ld ra, (sp)
  addi sp, sp, 0x8
  ret

_start1: /* a0 must hold current hard ID passed by bootloader */
         /* a1 must hold dtb address passed by bootloader */
.option push
.option norelax
1:auipc gp, %pcrel_hi(__global_pointer$)
  addi  gp, gp, %pcrel_lo(1b)
.option pop

  la sp, (_stack + STACK_SIZE)

  /* save the parameters passed */
  mv s0, a0 /* preserve a0 (hart id) in s0 */
  mv s2, a1 /* preserve a1 (dtb) in s2 */

  /* May not have been loaded in the right location.
   * Try and move ourselves so we're in the right place
   */
  jal relocate

  /* Check if we were relocated. If not, just jump straight to main */
  li a1, 1
  beq a0, a1, 1f

  /* Restore arguments and go back to _start of the relocated image */
  mv s3, a0
  mv a0, s0 /* restore a0 to hold hart ID passed by the boot loader */
  mv a1, s2 /* restore a1 to hold dtb address passed by the boot loader */
  jr s3

1:
  la s0, main
  jr s0

.text

.global secondary_harts
secondary_harts:

.option push
.option norelax
1:auipc gp, %pcrel_hi(__global_pointer$)
  addi  gp, gp, %pcrel_lo(1b)
.option pop

spin_hart:
  wfi
  j spin_hart

.global riscv_secondary_cpu_entry_asm
riscv_secondary_cpu_entry_asm:

.option push
.option norelax
1:auipc gp, %pcrel_hi(__global_pointer$)
  addi  gp, gp, %pcrel_lo(1b)
.option pop

  /* the HSM passes a0 as hartid, and a1 the 'opaque' value (our stack) */
  mv sp, a1
  /* retrieve our saved logical cpu and put in a1 */
  ld a1, 0(sp)
  la s0, riscv_secondary_cpu_entry
  jr s0
