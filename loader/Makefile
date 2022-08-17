#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
ifeq ($(strip $(BUILD_DIR)),)
$(error BUILD_DIR must be specified)
endif

# @ivanv: find a better solution than this, maybe gcc_cpu should be an optional arugment?
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

ifeq ($(strip $(TOOLCHAIN)),)
$(error TOOLCHAIN must be specified)
endif

ifeq ($(ARCH),aarch64)
	C_FLAGS := -std=gnu11 -g3 -O3 -nostdlib -ffreestanding -mcpu=$(GCC_CPU) -DBOARD_$(BOARD) -Wall -Werror
	ASM_CPP_FLAGS := -x assembler-with-cpp -c -g3 -mcpu=$(GCC_CPU)
	ASM_FLAGS := -g3 -mcpu=$(GCC_CPU)
else ifeq ($(ARCH),riscv64)
	# @ivanv: if we're going to have an optimised build, should we pass in -mtune as well?
	C_FLAGS := -std=gnu11 -g3 -O3 -nostdlib -ffreestanding -march=rv64im -mabi=lp64 -DBOARD_$(BOARD) -Wall -Werror
	ASM_CPP_FLAGS := -x assembler-with-cpp -c -g3 -march=rv64im -mabi=lp64
	ASM_FLAGS := -g3 -march=rv64im -mabi=lp64
else
	$(error ARCH must be aarch64 or riscv64)
endif

PROGS := loader.elf
OBJECTS := loader.o crt0.o util64.o
LINKSCRIPT_INPUT := loader.ld
LINKSCRIPT := $(BUILD_DIR)/link.ld

$(BUILD_DIR)/%.o : src/%.S
	$(TOOLCHAIN)gcc $(ASM_CPP_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/%.s
	$(TOOLCHAIN)as $(ASM_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/%.c
	$(TOOLCHAIN)gcc -c $(C_FLAGS) $< -o $@

OBJPROG = $(addprefix $(BUILD_DIR)/, $(PROGS))

all: $(OBJPROG)

$(LINKSCRIPT): $(LINKSCRIPT_INPUT)
	$(TOOLCHAIN)cpp -DLINK_ADDRESS=$(LINK_ADDRESS) $< | grep -v "^#" > $@

$(OBJPROG): $(addprefix $(BUILD_DIR)/, $(OBJECTS)) $(LINKSCRIPT)
	$(TOOLCHAIN)ld -T$(LINKSCRIPT) $(addprefix $(BUILD_DIR)/, $(OBJECTS)) -o $@
