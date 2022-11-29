/*
 * Copyright 2020, Data61, CSIRO (ABN 41 687 119 230)
 * Copyright 2022, UNSW (ABN 57 195 873 179)
 *
 * SPDX-License-Identifier: GPL-2.0-only
 */

/* Note: this was originally taken from the RISC-V bootloader in elfloader in seL4_tools */
// @ivanv: this file needs to be cleaned up

.extern main
.extern __global_pointer$

.section ".text.start"

#define STACK_SIZE 4096

/* SBI commands for the HSM extension */
#define SBI_HSM_BASE_EID 0x48534DULL
#define SBI_HSM_BASE_HART_START_FID 0
#define SBI_HSM_BASE_HART_STOP_FID 1
/* SBI commands for the base extension */
#define SBI_EXT_BASE_EID 0x10
#define SBI_EXT_BASE_PROBE_EXT_FID 3

/* This comes from the config.cmake script for the platform in seL4.
 * This is not the most ideal way to do this, however once we add the kernel
 * config it should be nicer. @ivanv
 */
#if defined(BOARD_spike)
#define FIRST_HART_ID 0
#elif defined(BOARD_hifive_unleashed)
#define FIRST_HART_ID 1
#else
#error "Need to define FIRST_HART_ID"
#endif

/* OpenSBI starts us these parameters:
 *   a0: hart id
 *   a1: dtb
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
  li a7, SBI_EXT_BASE_EID
  li a6, SBI_EXT_BASE_PROBE_EXT_FID
  li a0, SBI_HSM_BASE_EID
  ecall /* call SBI to probe for HSM extension */
  mv a2, a0 /* move SBI call generic return code to a2 as we need a0 */
  mv a3, a1 /* move SBI call error return code to a3 as we need a1 */
  mv a0, s0 /* restore a0 to hold hart ID passed by the boot loader */
  mv a1, s2 /* restore a1 to hold dtb address passed by the boot loader */
  bnez a2, _start1 /* goto _start1 if SBI did not return SBI_SUCCESS (0) */
  beqz a3, _start1 /* goto _start1 if HSM extension is missing */

  /* Check if we are running on the hart we expect on, FIRST_HART_ID */
  li s1, FIRST_HART_ID
  beq  a0, s1, _start1 /* goto _start1 if we are on FIRST_HART_ID */

  /* Use HSM extension to start hart FIRST_HART_ID. */
hsm_switch_hart:
  li a7, SBI_HSM_BASE_EID
  li a6, SBI_HSM_BASE_HART_START_FID
  li a0, FIRST_HART_ID
  mv a2, s2 /* dtb address to be passed in a1 when new hart starts is 3rd parameter */
  la a1, _start1 /* where to start the hart */
  ecall /* call SBI to start hart FIRST_HART_ID */

  /* Since we are not the designated primary hart, continue the boot process as
   * secondary hart
   */
  mv a0, s0 /* restore a0 to hold hart ID passed by OpenSBI */
  j spin_hart /* Spin any hart that isn't FIRST_HART_ID since we only support single-core right now. */


_start1: /* a0 must hold current hard ID passed by bootloader */
         /* a1 must hold dtb address passed by bootloader */
.option push
.option norelax
1:auipc gp, %pcrel_hi(__global_pointer$)
  addi  gp, gp, %pcrel_lo(1b)
.option pop

  la sp, (_stack + STACK_SIZE)
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

