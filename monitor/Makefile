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

ifeq ($(strip $(TOOLCHAIN)),)
$(error TOOLCHAIN must be specified)
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

CFLAGS := -std=gnu11 -g -O3 -nostdlib -ffreestanding -Wall -Wno-maybe-uninitialized -Werror -I$(SEL4_SDK)/include $(CFLAGS_ARCH) -DARCH_$(ARCH)

PROGS := monitor.elf
OBJECTS := main.o crt0.o debug.o util.o
LINKSCRIPT := monitor.ld

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.S
	$(TOOLCHAIN)gcc $(ASM_CPP_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.s
	$(TOOLCHAIN)as -g $(ASM_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/%.c
	$(TOOLCHAIN)gcc -c $(CFLAGS)  $< -o $@

OBJPROG = $(addprefix $(BUILD_DIR)/, $(PROGS))

all: $(OBJPROG)

$(OBJPROG): $(addprefix $(BUILD_DIR)/, $(OBJECTS)) $(LINKSCRIPT)
	$(TOOLCHAIN)ld -T$(LINKSCRIPT) $(addprefix $(BUILD_DIR)/, $(OBJECTS)) -o $@
