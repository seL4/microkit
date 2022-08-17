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

ifeq ($(strip $(ARCH)),)
$(error ARCH must be specified)
endif

ifeq ($(strip $(TOOLCHAIN)),)
$(error TOOLCHAIN must be specified)
endif

# @ivanv: This is, quite the mess. I dislike this, but at the same time I dislike the over abstractions of build systems like CMake...
ifeq ($(ARCH),aarch64)
	C_FLAGS := -std=gnu11 -g3 -O3 -nostdlib -ffreestanding -mcpu=$(GCC_CPU) -Wall -Wno-maybe-uninitialized -Wno-unused-function -Werror -Iinclude -I$(SEL4_SDK)/include
	ASM_CPP_FLAGS := -x assembler-with-cpp -c -g3 -mcpu=$(GCC_CPU)
	ASM_FLAGS := -g3 -mcpu=$(GCC_CPU)
else ifeq ($(ARCH),riscv64)
	# @ivanv: if we're going to have an optimised build, should we pass in -mtune as well?
	C_FLAGS := -std=gnu11 -g3 -O3 -nostdlib -ffreestanding -march=rv64im -mabi=lp64 -Wall -Wno-maybe-uninitialized -Wno-unused-function -Werror -Iinclude -I$(SEL4_SDK)/include
	ASM_CPP_FLAGS := -x assembler-with-cpp -c -g3 -march=rv64im -mabi=lp64
	ASM_FLAGS := -g3 -march=rv64im -mabi=lp64
else
	$(error ARCH must be aarch64 or riscv64)
endif

LIBS := libsel4cp.a
OBJS := main.o dbg.o
OTHER_OBJS := crt0.o

$(BUILD_DIR)/%.o : src/%.S
	$(TOOLCHAIN)gcc $(ASM_CPP_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/%.s
	$(TOOLCHAIN)as $(ASM_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/%.c
	$(TOOLCHAIN)gcc -c $(C_FLAGS) $< -o $@

LIB = $(addprefix $(BUILD_DIR)/, $(LIBS))

all: $(LIB) $(addprefix $(BUILD_DIR)/, $(OTHER_OBJS))

$(LIB): $(addprefix $(BUILD_DIR)/, $(OBJS))
	$(TOOLCHAIN)ar -rv $@ $^
