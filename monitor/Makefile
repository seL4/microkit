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
	C_FLAGS := -std=gnu11 -g -O3 -nostdlib -ffreestanding -mcpu=$(GCC_CPU) -DARCH_aarch64 -Wall -Wno-maybe-uninitialized -Werror -I$(SEL4_SDK)/include
	ASM_CPP_FLAGS := -x assembler-with-cpp -c -g -mcpu=$(GCC_CPU)
	ASM_FLAGS := -g -mcpu=$(GCC_CPU)
else ifeq ($(ARCH),riscv64)
	C_FLAGS := -mcmodel=medany -std=gnu11 -g -O3 -nostdlib -ffreestanding -march=rv64imac -mabi=lp64 -DARCH_riscv64 -Wall -Wno-maybe-uninitialized -Werror -I$(SEL4_SDK)/include
	ASM_CPP_FLAGS := -x assembler-with-cpp -c -g -march=rv64imac -mabi=lp64
	ASM_FLAGS := -g -march=rv64imac -mabi=lp64
else ifeq ($(ARCH),riscv32)
	C_FLAGS := -mcmodel=medany -std=gnu11 -g -O3 -nostdlib -ffreestanding -march=rv32imac -mabi=ilp32 -DARCH_riscv32 -Wall -Wno-maybe-uninitialized -Werror -I$(SEL4_SDK)/include
	ASM_CPP_FLAGS := -x assembler-with-cpp -c -g -march=rv32imac -mabi=ilp32
	ASM_FLAGS := -g -march=rv32imac -mabi=ilp32
else
	$(error ARCH must be aarch64 or riscv64 or riscv32)
endif

ifeq ($(ARCH),x86_64)
	GCC := gcc
	AS := as
else
	GCC := $(TOOLCHAIN)gcc
	AS := $(TOOLCHAIN)as
endif

PROGS := monitor.elf
OBJECTS := main.o crt0.o debug.o util.o
LINKSCRIPT := monitor.ld

$(BUILD_DIR)/%.o : src/$(ARCH)/%.S
	$(GCC) $(ASM_CPP_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/$(ARCH)/%.s
	$(AS) $(ASM_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/$(ARCH)/%.c
	$(GCC) -c $(C_FLAGS)  $< -o $@

$(BUILD_DIR)/%.o : src/%.S
	$(GCC) $(ASM_CPP_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/%.s
	$(AS) $(ASM_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/%.c
	$(GCC) -c $(C_FLAGS)  $< -o $@

OBJPROG = $(addprefix $(BUILD_DIR)/, $(PROGS))

all: $(OBJPROG)

$(OBJPROG): $(addprefix $(BUILD_DIR)/, $(OBJECTS)) $(LINKSCRIPT)
	$(TOOLCHAIN)ld -T$(LINKSCRIPT) $(addprefix $(BUILD_DIR)/, $(OBJECTS)) -o $@
