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
CFLAGS := -std=gnu11 -g3 -O3 -nostdlib -ffreestanding -mcpu=$(GCC_CPU) -Wall -Werror

PROGS := loader.elf
OBJECTS := loader.o crt0.o util64.o
LINKSCRIPT := loader.ld

$(BUILD_DIR)/%.o : src/%.S
	$(TOOLCHAIN)gcc -x assembler-with-cpp -c -g3  -mcpu=$(GCC_CPU)  $< -o $@

$(BUILD_DIR)/%.o : src/%.s
	$(TOOLCHAIN)as -g3 -mcpu=$(GCC_CPU) $< -o $@

$(BUILD_DIR)/%.o : src/%.c
	$(TOOLCHAIN)gcc -c $(CFLAGS)  $< -o $@

OBJPROG = $(addprefix $(BUILD_DIR)/, $(PROGS))

all: $(OBJPROG)

$(OBJPROG): $(addprefix $(BUILD_DIR)/, $(OBJECTS))
	$(TOOLCHAIN)ld -T$(LINKSCRIPT) $^ -o $@
