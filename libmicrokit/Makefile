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
  CC := clang -target $(TARGET_TRIPLE)
  CPP := clang-cpp -target $(TARGET_TRIPLE)
  AS := clang -target $(TARGET_TRIPLE)
  LD := ld.lld
  AR := llvm-ar
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
	ASM_FLAGS := -mcpu=$(GCC_CPU)
	CFLAGS_AARCH64 := -mcpu=$(GCC_CPU)
	CFLAGS_ARCH := $(CFLAGS_AARCH64)
	ARCH_DIR := aarch64
else ifeq ($(ARCH),riscv64)
	ASM_FLAGS := -march=rv64imafdc_zicsr_zifencei -mabi=lp64d
	CFLAGS_RISCV64 := -mcmodel=medany -march=rv64imafdc_zicsr_zifencei -mabi=lp64d
	CFLAGS_ARCH := $(CFLAGS_RISCV64)
	ARCH_DIR := riscv
else ifeq ($(ARCH),x86_64)
	CFLAGS_ARCH := -march=x86-64 -mtune=$(GCC_CPU)
	ASM_FLAGS := -march=generic64
	ARCH_DIR := x86_64
endif

CFLAGS := -std=gnu11 \
		  -g -O3 -nostdlib \
		  -ffreestanding \
		  -Wall $(CFLAGS_TOOLCHAIN) \
		  -Wno-unused-function -Werror \
		  -Iinclude -I$(SEL4_SDK)/include \
		  $(CFLAGS_ARCH)

LIBS := libmicrokit.a
OBJS := main.o crt0.o dbg.o

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.S
	$(CC) -x assembler-with-cpp -c $(CFLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.s
	$(AS) -c -g $(ASM_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/%.c
	$(CC) -c $(CFLAGS) $< -o $@

LIB = $(addprefix $(BUILD_DIR)/, $(LIBS))

all: $(LIB)

$(LIB): $(addprefix $(BUILD_DIR)/, $(OBJS))
	$(AR) -rv $@ $^
