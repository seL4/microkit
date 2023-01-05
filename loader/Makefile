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

ifeq ($(strip $(BOARD)),)
$(error BOARD must be specified)
endif

ifeq ($(strip $(LINK_ADDRESS)),)
$(error LINK_ADDRESS must be specified)
endif

ifeq ($(strip $(ARCH)),)
$(error ARCH must be specified)
endif

# ifeq ($(strip $(TOOLCHAIN)),)
# $(error TOOLCHAIN must be specified)
# endif

ifeq ($(strip $(NUM_CPUS)),)
$(error NUM_CPUS must be specified)
endif

ifeq ($(ARCH),x86_64)
	GCC := gcc
	AS := as
	CPP := cpp
	ld := ld
else
	GCC := $(TOOLCHAIN)gcc
	AS := $(TOOLCHAIN)as
	CPP := $(TOOLCHAIN)cpp
	LD := $(TOOLCHAIN)ld
endif

ifeq ($(ARCH),aarch64)
	# FIXME @ivanv: investigate why having -O3 causes QEMU ARM virt to not be able to boot.
	C_FLAGS_ARCH := -mcpu=$(GCC_CPU)
	ASM_CPP_FLAGS_ARCH := -mcpu=$(GCC_CPU)
	ASM_FLAGS_ARCH := -mcpu=$(GCC_CPU)
else ifeq ($(ARCH),riscv64)
	MARCH := rv64imac
	MABI := lp64
	C_FLAGS_ARCH := -mcmodel=medany -march=$(MARCH) -mabi=$(MABI)
	ASM_CPP_FLAGS_ARCH := -march=$(MARCH) -mabi=$(MABI)
	ASM_FLAGS_ARCH := -march=$(MARCH) -mabi=$(MABI)
else ifeq ($(ARCH),riscv32)
	MARCH := rv32imac
	MABI := ilp32
	C_FLAGS_ARCH := -mcmodel=medany -march=$(MARCH) -mabi=$(MABI)
	ASM_CPP_FLAGS_ARCH := -march=$(MARCH) -mabi=$(MABI)
	ASM_FLAGS_ARCH := -march=$(MARCH) -mabi=$(MABI)
	LD_FLAGS_ARCH := -m elf32lriscv
else ifeq ($(ARCH),x86_64)
	C_FLAGS := -std=gnu11 -g3 -O3 -nostdlib -ffreestanding -DBOARD_$(BOARD) -DARCH_x86_64 -Wall -Werror
	ASM_CPP_FLAGS := -x assembler-with-cpp -c -g3 -DBOARD_$(BOARD)
	ASM_FLAGS := -g3
else
	$(error ARCH must be aarch64 or riscv64 or riscv32)
endif

C_FLAGS := -std=gnu11 -g3 -nostdlib -ffreestanding -DBOARD_$(BOARD) -DNUM_CPUS=$(NUM_CPUS) -DARCH_$(ARCH) -Wall -Werror $(C_FLAGS_ARCH)
ASM_CPP_FLAGS := -x assembler-with-cpp -c -g3 -DBOARD_$(BOARD) -DNUM_CPUS=$(NUM_CPUS) $(ASM_CPP_FLAGS_ARCH)
ASM_FLAGS := -g3 $(ASM_FLAGS_ARCH)
LD_FLAGS := $(LD_FLAGS_ARCH)

PROGS := loader.elf

LINKSCRIPT_INPUT := loader.ld
LINKSCRIPT := $(BUILD_DIR)/link.ld
ifeq ($(ARCH),aarch64)
	OBJECTS := loader.o crt0.o util64.o
	ARCH_DIR := aarch64
else ifeq ($(ARCH),riscv64)
	OBJECTS := loader.o crt0.o
	ARCH_DIR := riscv
else ifeq ($(ARCH),riscv32)
	OBJECTS := loader.o crt0.o
	ARCH_DIR := riscv
else
	$(error ARCH must be aarch64 or riscv64 or riscv32)
endif

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.S
	$(GCC) $(ASM_CPP_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.s
	$(AS) $(ASM_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.c
	$(GCC) -c $(C_FLAGS) $< -o $@

OBJPROG = $(addprefix $(BUILD_DIR)/, $(PROGS))

all: $(OBJPROG)

$(LINKSCRIPT): src/$(ARCH_DIR)/$(LINKSCRIPT_INPUT)
	$(CPP) -DLINK_ADDRESS=$(LINK_ADDRESS) $< | grep -v "^#" > $@

$(OBJPROG): $(addprefix $(BUILD_DIR)/, $(OBJECTS)) $(LINKSCRIPT)
	$(LD) $(LD_FLAGS) -T$(LINKSCRIPT) $(addprefix $(BUILD_DIR)/, $(OBJECTS)) -o $@
