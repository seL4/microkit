#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
ifeq ($(strip $(BUILD_DIR)),)
$(error BUILD_DIR must be specified)
endif

ifeq ($(strip $(GCC_CPU)),)
$(error GCC_CPU must be specified)
endif

TOOLCHAIN := aarch64-none-elf-
CFLAGS := -std=gnu11 -g3 -O3 -nostdlib -ffreestanding -mcpu=$(GCC_CPU) -Wall -Wno-maybe-uninitialized -Wno-unused-function -Werror -Iinclude -I$(SEL4_SDK)/include

LIBS := libmicrokit.a
OBJS := main.o dbg.o
OTHER_OBJS :=  crt0.o

$(BUILD_DIR)/%.o : src/%.S
	$(TOOLCHAIN)gcc -x assembler-with-cpp -c -g3  -mcpu=$(GCC_CPU)  $< -o $@

$(BUILD_DIR)/%.o : src/%.s
	$(TOOLCHAIN)as -g -mcpu=$(GCC_CPU) $< -o $@

$(BUILD_DIR)/%.o : src/%.c
	$(TOOLCHAIN)gcc -c $(CFLAGS) $< -o $@

LIB = $(addprefix $(BUILD_DIR)/, $(LIBS))

all: $(LIB) $(addprefix $(BUILD_DIR)/, $(OTHER_OBJS))

$(LIB): $(addprefix $(BUILD_DIR)/, $(OBJS))
	$(TOOLCHAIN)ar -rv $@ $^
