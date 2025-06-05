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

ifeq ($(strip $(TARGET_TRIPLE)),)
$(error TARGET_TRIPLE must be specified)
endif

ifeq ($(strip $(LLVM)),True)
  CC = clang -target $(TARGET_TRIPLE)
  CPP = clang-cpp -target $(TARGET_TRIPLE)
  AS = clang -target $(TARGET_TRIPLE)
  LD = ld.lld
  AR = llvm-ar
  CFLAGS_TOOLCHAIN :=
else
  CC = $(TARGET_TRIPLE)-gcc
  CPP = $(TARGET_TRIPLE)-cpp
  AS = $(TARGET_TRIPLE)-as
  LD = $(TARGET_TRIPLE)-ld
  AR = $(TARGET_TRIPLE)-ar
  CFLAGS_TOOLCHAIN := -Wno-maybe-uninitialized
endif

ifeq ($(ARCH),aarch64)
	CFLAGS_ARCH := -mcpu=$(GCC_CPU)
	ASM_CPP_FLAGS := -x assembler-with-cpp -c -g -mcpu=$(GCC_CPU)
	ASM_FLAGS := -mcpu=$(GCC_CPU)

	ARCH_DIR := aarch64
else ifeq ($(ARCH),riscv64)
	ASM_CPP_FLAGS := -x assembler-with-cpp -c -g -march=rv64imac_zicsr_zifencei -mabi=lp64
	ASM_FLAGS := -march=rv64imac_zicsr_zifencei -mabi=lp64
	CFLAGS_ARCH := -mcmodel=medany -march=rv64imac_zicsr_zifencei -mabi=lp64 -DARCH_riscv64

	ARCH_DIR := riscv
else
	$(error ARCH is unsupported)
endif

CFLAGS := -std=gnu11 -g -O3 -nostdlib -ffreestanding -Wall $(CFLAGS_TOOLCHAIN) -Werror -I$(SEL4_SDK)/include $(CFLAGS_ARCH) -DARCH_$(ARCH)

PROGS := monitor.elf
OBJECTS := main.o crt0.o debug.o util.o
LINKSCRIPT := monitor.ld

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.S
	$(CC) $(ASM_CPP_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.s
	$(AS) -c -g $(ASM_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/%.c
	$(CC) -c $(CFLAGS)  $< -o $@

OBJPROG = $(addprefix $(BUILD_DIR)/, $(PROGS))

all: $(OBJPROG)

$(OBJPROG): $(addprefix $(BUILD_DIR)/, $(OBJECTS)) $(LINKSCRIPT)
	$(LD) -T$(LINKSCRIPT) $(addprefix $(BUILD_DIR)/, $(OBJECTS)) -o $@
