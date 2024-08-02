#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
ifeq ($(strip $(BUILD_DIR)),)
$(error BUILD_DIR must be specified)
endif

ifeq ($(strip $(ARCH)),)
$(error ARCH must be specified)
endif

ifeq ($(strip $(BOARD)),)
$(error BOARD must be specified)
endif

ifeq ($(strip $(LINK_ADDRESS)),)
$(error LINK_ADDRESS must be specified)
endif

ifeq ($(strip $(TOOLCHAIN)),)
$(error TOOLCHAIN must be specified)
endif

ifeq ($(strip $(PRINTING)),)
$(error PRINTING must be specified)
endif

ifeq ($(ARCH),aarch64)
	CFLAGS_AARCH64 := -DPHYSICAL_ADDRESS_BITS=$(PHYSICAL_ADDRESS_BITS) -mcpu=$(GCC_CPU) -mgeneral-regs-only
	CFLAGS_ARCH := $(CFLAGS_AARCH64) -DARCH_aarch64
	ASM_FLAGS_ARCH := -DPHYSICAL_ADDRESS_BITS=$(PHYSICAL_ADDRESS_BITS) -mcpu=$(GCC_CPU)
	ARCH_DIR := aarch64
else ifeq ($(ARCH),riscv64)
	CFLAGS_RISCV64 := -mcmodel=medany -march=rv64imac_zicsr_zifencei -mabi=lp64
	CFLAGS_ARCH := $(CFLAGS_RISCV64) -DARCH_riscv64
	ASM_FLAGS_ARCH := -march=rv64imac_zicsr_zifencei -mabi=lp64 -DFIRST_HART_ID=$(FIRST_HART_ID)
	ARCH_DIR := riscv
endif

CFLAGS := -std=gnu11 -g -O3 -nostdlib -ffreestanding $(CFLAGS_ARCH) -DBOARD_$(BOARD) -DPRINTING=$(PRINTING) -Wall -Werror -Wno-unused-function

ASM_FLAGS := $(ASM_FLAGS_ARCH) -g

PROGS := loader.elf
OBJECTS := loader.o crt0.o

ifeq ($(ARCH),aarch64)
	OBJECTS += util64.o
endif

LINKSCRIPT_INPUT := $(ARCH).ld
LINKSCRIPT := $(BUILD_DIR)/link.ld

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.S
	$(TOOLCHAIN)gcc -x assembler-with-cpp -c $(ASM_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.s
	$(TOOLCHAIN)as $< -o $@

$(BUILD_DIR)/%.o : src/%.c
	$(TOOLCHAIN)gcc -c $(CFLAGS) $< -o $@

OBJPROG = $(addprefix $(BUILD_DIR)/, $(PROGS))

all: $(OBJPROG)

$(LINKSCRIPT): $(LINKSCRIPT_INPUT)
	$(TOOLCHAIN)cpp -DLINK_ADDRESS=$(LINK_ADDRESS) $< | grep -v "^#" > $@

$(OBJPROG): $(addprefix $(BUILD_DIR)/, $(OBJECTS)) $(LINKSCRIPT)
	$(TOOLCHAIN)ld -T$(LINKSCRIPT) $(addprefix $(BUILD_DIR)/, $(OBJECTS)) -o $@
