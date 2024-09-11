/*
 * Copyright 2023, Neutrality.
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

/*
 * Note that a lot of this is taken from seL4. Some fields from seL4's
 * multiboot structure are configurable and we should make sure that we use the
 * same values here because we'll hand over the multiboot information data
 * (mostly) untouched to seL4.
 */

#include "multiboot.h"

#ifdef CONFIG_MULTIBOOT_GRAPHICS_MODE_NONE
#  define MBT1_FLAGS   (MULTIBOOT1_HEADER_FLAG_PAGE_ALIGN | MULTIBOOT1_HEADER_FLAG_REQ_MEMORY)
#  define MBT1_GFXMODE MULTIBOOT1_HEADER_GFX_MODE_GFX
#else
#  define MBT1_FLAGS   (MULTIBOOT1_HEADER_FLAG_PAGE_ALIGN | MULTIBOOT1_HEADER_FLAG_REQ_MEMORY | MULTIBOOT1_HEADER_FLAG_REQ_VIDEO)
#  ifdef CONFIG_MULTIBOOT_GRAPHICS_MODE_TEXT
#    define MBT1_GFXMODE MULTIBOOT1_HEADER_GFX_MODE_TEXT
#  else
#    define MBT1_GFXMODE MULTIBOOT1_HEADER_GFX_MODE_GFX
#  endif
#endif

#ifndef CONFIG_MULTIBOOT_GRAPHICS_MODE_HEIGHT
#define CONFIG_MULTIBOOT_GRAPHICS_MODE_HEIGHT 0
#endif
#ifndef CONFIG_MULTIBOOT_GRAPHICS_MODE_WIDTH
#define CONFIG_MULTIBOOT_GRAPHICS_MODE_WIDTH 0
#endif
#ifndef CONFIG_MULTIBOOT_GRAPHICS_MODE_DEPTH
#define CONFIG_MULTIBOOT_GRAPHICS_MODE_DEPTH 0
#endif

.section .text.start
.code32

#ifdef CONFIG_MULTIBOOT1_HEADER
	.align	4
	.long	MULTIBOOT1_HEADER_MAGIC                 /* magic                */
	.long	MBT1_FLAGS                              /* flags                */
	.long	-(MBT1_FLAGS + MULTIBOOT1_HEADER_MAGIC) /* checksum             */
	.long	0                                       /* header_addr          */
	.long	0                                       /* load_addr            */
	.long	0                                       /* load_end_addr        */
	.long	0                                       /* bss_end_addr         */
	.long	0                                       /* entry_addr           */
	.long	MBT1_GFXMODE                            /* mode_type            */
	.long	CONFIG_MULTIBOOT_GRAPHICS_MODE_WIDTH    /* width                */
	.long	CONFIG_MULTIBOOT_GRAPHICS_MODE_HEIGHT   /* height               */
	.long	CONFIG_MULTIBOOT_GRAPHICS_MODE_DEPTH    /* depth                */
#endif

#ifdef CONFIG_MULTIBOOT2_HEADER
#define MBT2_SIZE       (__mbi2_end - __mbi2_start)
	.align	8
__mbi2_start:
	.long	MULTIBOOT2_HEADER_MAGIC                 /* magic                */
	.long	0                                       /* architecture (i386)  */
	.long	MBT2_SIZE                               /* header size          */
	.long	-(MULTIBOOT2_HEADER_MAGIC + MBT2_SIZE)  /* checksum             */
	.word	0x0                                     /* end tag: type        */
	.word	0x0                                     /* end tag: flags       */
	.long	0x8                                     /* end tag: size        */
__mbi2_end:
#endif
