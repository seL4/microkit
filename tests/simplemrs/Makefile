#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
LIBSEL4CP := ../../libsel4cp
TOOLCHAIN := aarch64-linux-gnu
PYTHON := python3.8

CPU := cortex-a35

CC := $(TOOLCHAIN)-gcc
LD := $(TOOLCHAIN)-ld
AS := $(TOOLCHAIN)-as

SIMPLEMRS_OBJS := simplemrs.o

IMAGES := simplemrs.elf
CFLAGS := -mcpu=$(CPU) -mstrict-align -nostdlib -ffreestanding -g3 -O3 -Wall  -Wno-unused-function -Werror -I$(SEL4_SDK)/include -I$(LIBSEL4CP)/include
LDFLAGS := -L$(LIBSEL4CP) -Tsel4cp.ld
LIBS := -lsel4cp


all: $(IMAGES)

%.o: %.c Makefile
	$(CC) -c $(CFLAGS) $< -o $@

%.o: %.s Makefile
	$(AS) -g3 -mcpu=$(CPU) $< -o $@

simplemrs.elf: $(SIMPLEMRS_OBJS)
	$(LD) $(LDFLAGS) $(SIMPLEMRS_OBJS) $(LIBS) -o $@
