#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
ifeq ($(strip $(BUILD_DIR)),)
$(error BUILD_DIR must be specified)
endif

# ifeq ($(strip $(GCC_CPU)),)
# $(error GCC_CPU must be specified)
# endif

ifeq ($(strip $(ARCH)),)
$(error ARCH must be specified)
endif

# ifeq ($(strip $(TOOLCHAIN)),)
# $(error TOOLCHAIN must be specified)
# endif

ifeq ($(ARCH),aarch64)
	C_FLAGS_ARCH := -mcpu=$(GCC_CPU) -mgeneral-regs-only
	ASM_CPP_FLAGS_ARCH := -mcpu=$(GCC_CPU)
	ASM_FLAGS_ARCH := -mcpu=$(GCC_CPU)
	ARCH_DIR := aarch64
else ifeq ($(ARCH),riscv64)
	MARCH := rv64imac
	MABI := lp64
	C_FLAGS_ARCH := -mcmodel=medany -march=$(MARCH) -mabi=$(MABI)
	ASM_CPP_FLAGS_ARCH := -march=$(MARCH) -mabi=$(MABI)
	ASM_FLAGS_ARCH := -g3 -march=$(MARCH) -mabi=$(MABI)
	ARCH_DIR := riscv
else ifeq ($(ARCH),riscv32)
	MARCH := rv32imac
	MABI := ilp32
	C_FLAGS_ARCH := -mcmodel=medany -march=$(MARCH) -mabi=$(MABI)
	ASM_CPP_FLAGS_ARCH := -march=$(MARCH) -mabi=$(MABI)
	ASM_FLAGS_ARCH := -g3 -march=$(MARCH) -mabi=$(MABI)
	ARCH_DIR := riscv
else
	$(error ARCH must be aarch64 or riscv64 or riscv32)
endif

C_FLAGS := -std=gnu11 -g3 -O3 -nostdlib -ffreestanding -Wall -Wno-maybe-uninitialized -Wno-unused-function -Werror -Iinclude -I$(SEL4_SDK)/include $(C_FLAGS_ARCH)
ASM_CPP_FLAGS := -x assembler-with-cpp -c -g3 $(ASM_CPP_FLAGS_ARCH)
ASM_FLAGS := -g3 $(ASM_FLAGS_ARCH)

ifeq ($(ARCH),x86_64)
	GCC := gcc
	AS := as
else
	GCC := $(TOOLCHAIN)gcc
	AS := $(TOOLCHAIN)as
endif

LIBS := libsel4cp.a
OBJS := main.o dbg.o
OTHER_OBJS := crt0.o

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.S
	$(GCC) $(ASM_CPP_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.s
	$(AS) $(ASM_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.c
	$(GCC) -c $(C_FLAGS)  $< -o $@

$(BUILD_DIR)/%.o : src/%.S
	$(GCC) $(ASM_CPP_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/%.s
	$(AS) $(ASM_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/%.c
	$(GCC) -c $(C_FLAGS) $< -o $@

LIB = $(addprefix $(BUILD_DIR)/, $(LIBS))

all: $(LIB) $(addprefix $(BUILD_DIR)/, $(OTHER_OBJS))

$(LIB): $(addprefix $(BUILD_DIR)/, $(OBJS))
	$(TOOLCHAIN)ar -rv $@ $^
