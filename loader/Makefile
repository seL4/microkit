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

ifeq ($(strip $(BOARD)),)
$(error BOARD must be specified)
endif

ifeq ($(strip $(LINK_ADDRESS)),)
$(error LINK_ADDRESS must be specified)
endif

ifeq ($(strip $(PHYSICAL_ADDRESS_BITS)),)
$(error PHYSICAL_ADDRESS_BITS must be specified)
endif

TOOLCHAIN := aarch64-none-elf-
CFLAGS := -std=gnu11 -g -O3 -nostdlib -ffreestanding -mcpu=$(GCC_CPU) -DBOARD_$(BOARD) -Wall -Werror -mgeneral-regs-only

PROGS := loader.elf
OBJECTS := loader.o crt0.o util64.o
LINKSCRIPT_INPUT := loader.ld
LINKSCRIPT := $(BUILD_DIR)/link.ld

$(BUILD_DIR)/%.o : src/%.S
	$(TOOLCHAIN)gcc -x assembler-with-cpp -c -g -DPHYSICAL_ADDRESS_BITS=$(PHYSICAL_ADDRESS_BITS) -mcpu=$(GCC_CPU)  $< -o $@

$(BUILD_DIR)/%.o : src/%.s
	$(TOOLCHAIN)as -g -mcpu=$(GCC_CPU) $< -o $@

$(BUILD_DIR)/%.o : src/%.c
	$(TOOLCHAIN)gcc -c $(CFLAGS)  $< -o $@

OBJPROG = $(addprefix $(BUILD_DIR)/, $(PROGS))

all: $(OBJPROG)

$(LINKSCRIPT): $(LINKSCRIPT_INPUT)
	$(TOOLCHAIN)cpp -DLINK_ADDRESS=$(LINK_ADDRESS) $< | grep -v "^#" > $@

$(OBJPROG): $(addprefix $(BUILD_DIR)/, $(OBJECTS)) $(LINKSCRIPT)
	$(TOOLCHAIN)ld -T$(LINKSCRIPT) $(addprefix $(BUILD_DIR)/, $(OBJECTS)) -o $@
