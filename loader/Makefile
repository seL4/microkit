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

ifeq ($(strip $(TARGET_TRIPLE)),)
$(error TARGET_TRIPLE must be specified)
endif

ifeq ($(strip $(LLVM)),True)
  CC = clang -target $(TARGET_TRIPLE)
  CPP = clang-cpp -target $(TARGET_TRIPLE)
  AS = clang -target $(TARGET_TRIPLE)
  LD = ld.lld
else
  CC = $(TARGET_TRIPLE)-gcc
  CPP = $(TARGET_TRIPLE)-cpp
  AS = $(TARGET_TRIPLE)-as
  LD = $(TARGET_TRIPLE)-ld
endif

ifeq ($(ARCH),aarch64)
	CFLAGS_AARCH64 := -mcpu=$(GCC_CPU) -mgeneral-regs-only -mstrict-align
	CFLAGS_ARCH := $(CFLAGS_AARCH64) -DARCH_aarch64
	ASM_FLAGS_ARCH := -mcpu=$(GCC_CPU)
	ARCH_DIR := aarch64
else ifeq ($(ARCH),riscv64)
	CFLAGS_RISCV64 := -mcmodel=medany -march=rv64imac_zicsr_zifencei -mabi=lp64
	CFLAGS_ARCH := $(CFLAGS_RISCV64) -DARCH_riscv64
	ASM_FLAGS_ARCH := -march=rv64imac_zicsr_zifencei -mabi=lp64
	ARCH_DIR := riscv
endif

CFLAGS := -std=gnu11 -g -O3 -nostdlib -ffreestanding -MP -MD $(CFLAGS_ARCH) -Wall -Werror -I$(SEL4_SDK)/include

ASM_FLAGS := $(ASM_FLAGS_ARCH) -g -MP -MD -I$(SEL4_SDK)/include

PROGS := loader.elf
OBJECTS := loader.o crt0.o uart.o cutil.o

ifeq ($(ARCH),aarch64)
	OBJECTS += util64.o el.o exceptions.o init.o mmu.o
else ifeq ($(ARCH),riscv64)
	OBJECTS += exceptions.o init.o mmu.o
endif

LINKSCRIPT_INPUT := $(ARCH).ld
LINKSCRIPT := $(BUILD_DIR)/link.ld

.SECONDARY:

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.S
	$(CC) -DLINK_ADDRESS=$(LINK_ADDRESS) -x assembler-with-cpp -c $(ASM_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.s
	$(AS) -c $< -o $@

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.c
	$(CC) -c $(CFLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/%.c
	$(CC) -c $(CFLAGS) $< -o $@

-include $(BUILD_DIR)/*.d

OBJPROG = $(addprefix $(BUILD_DIR)/, $(PROGS))

all: $(OBJPROG)

$(LINKSCRIPT): $(LINKSCRIPT_INPUT)
	$(CPP) -DLINK_ADDRESS=$(LINK_ADDRESS) $< | grep -v "^#" > $@

$(OBJPROG): $(addprefix $(BUILD_DIR)/, $(OBJECTS)) $(LINKSCRIPT)
	$(LD) -T$(LINKSCRIPT) $(addprefix $(BUILD_DIR)/, $(OBJECTS)) -o $@
