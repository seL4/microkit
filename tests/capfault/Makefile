#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
LIBMICROKIT := ../../libmicrokit
TOOLCHAIN := aarch64-linux-gnu
PYTHON := python3.8

CPU := cortex-a35

CC := $(TOOLCHAIN)-gcc
LD := $(TOOLCHAIN)-ld
AS := $(TOOLCHAIN)-as

CAPFAULT_OBJS := capfault.o

IMAGES := capfault.elf
CFLAGS := -mcpu=$(CPU) -mstrict-align -nostdlib -ffreestanding -g3 -O3 -Wall  -Wno-unused-function -Werror -I$(SEL4_SDK)/include -I$(LIBMICROKIT)/include
LDFLAGS := -L$(LIBMICROKIT) -Tmicrokit.ld
LIBS := -lmicrokit


all: $(IMAGES)

%.o: %.c Makefile
	$(CC) -c $(CFLAGS) $< -o $@

%.o: %.s Makefile
	$(AS) -g3 -mcpu=$(CPU) $< -o $@

capfault.elf: $(CAPFAULT_OBJS)
	$(LD) $(LDFLAGS) $(CAPFAULT_OBJS) $(LIBS) -o $@
