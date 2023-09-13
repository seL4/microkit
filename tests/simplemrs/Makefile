#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
LIBMICROKIT := ../../libmicrokit
TOOLCHAIN := aarch64-linux-gnu

CPU := cortex-a53

CC := $(TOOLCHAIN)-gcc
LD := $(TOOLCHAIN)-ld
AS := $(TOOLCHAIN)-as

SIMPLEMRS_OBJS := simplemrs.o

IMAGES := simplemrs.elf
CFLAGS := -mcpu=$(CPU) -mstrict-align -nostdlib -ffreestanding -g3 -O3 -Wall  -Wno-unused-function -Werror -I$(SEL4_SDK)/include -I$(LIBMICROKIT)/include
LDFLAGS := -L$(LIBMICROKIT) -Tmicrokit.ld
LIBS := -lmicrokit


all: $(IMAGES)

%.o: %.c Makefile
	$(CC) -c $(CFLAGS) $< -o $@

%.o: %.s Makefile
	$(AS) -g3 -mcpu=$(CPU) $< -o $@

simplemrs.elf: $(SIMPLEMRS_OBJS)
	$(LD) $(LDFLAGS) $(SIMPLEMRS_OBJS) $(LIBS) -o $@
