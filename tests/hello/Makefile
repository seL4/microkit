#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
# Copyright 2022, UNSW (ABN 57 195 873 179)
#
# SPDX-License-Identifier: BSD-2-Clause
#
ifeq ($(strip $(BUILD_DIR)),)
$(error BUILD_DIR must be specified)
endif

# ifeq ($(strip $(OPENSBI)),)
# $(error OPENSBI must be specified)
# endif

ifeq ($(strip $(SEL4CP_SDK)),)
$(error SEL4CP_SDK must be specified)
endif

ifeq ($(strip $(SEL4CP_BOARD)),)
$(error SEL4CP_BOARD must be specified)
endif

ifeq ($(strip $(SEL4CP_CONFIG)),)
$(error SEL4CP_CONFIG must be specified)
endif

IMAGE_FILE = $(BUILD_DIR)/$(IMAGE_NAME)
REPORT_FILE = $(BUILD_DIR)/report.txt

ifeq ($(ARCH),aarch64)
# 	C_FLAGS_ARCH := -mcpu=$(GCC_CPU)
# 	ASM_CPP_FLAGS_ARCH := -mcpu=$(GCC_CPU)
# 	ASM_FLAGS_ARCH := -mcpu=$(GCC_CPU)
	TOOLCHAIN := aarch64-none-elf
else ifeq ($(ARCH),riscv64)
	MARCH := rv64imac
	MABI := lp64
	C_FLAGS_ARCH := -mcmodel=medany -march=$(MARCH) -mabi=$(MABI)
	TOOLCHAIN := riscv64-unknown-elf
endif

CC := $(TOOLCHAIN)-gcc
LD := $(TOOLCHAIN)-ld
AS := $(TOOLCHAIN)-as
SEL4CP_TOOL ?= $(SEL4CP_SDK)/bin/sel4cp

HELLO_OBJS := hello.o

BOARD_DIR := $(SEL4CP_SDK)/board/$(SEL4CP_BOARD)/$(SEL4CP_CONFIG)

IMAGES := hello.elf
CFLAGS := -mstrict-align -nostdlib -ffreestanding -g3 -O3 -Wall -Wno-unused-function -Werror -I$(BOARD_DIR)/include $(C_FLAGS_ARCH)
LDFLAGS := -L$(BOARD_DIR)/lib
LIBS := -lsel4cp -Tsel4cp.ld

all: $(IMAGE_FILE)

$(BUILD_DIR)/%.o: %.c Makefile
	$(CC) -c $(CFLAGS) $< -o $@

$(BUILD_DIR)/hello.elf: $(addprefix $(BUILD_DIR)/, $(HELLO_OBJS))
	$(LD) $(LDFLAGS) $^ $(LIBS) -o $@

$(IMAGE_FILE) $(REPORT_FILE): $(addprefix $(BUILD_DIR)/, $(IMAGES)) hello.system
	$(SEL4CP_TOOL) hello.system --search-path $(BUILD_DIR) --board $(SEL4CP_BOARD) --config $(SEL4CP_CONFIG) -o $(IMAGE_FILE) -r $(REPORT_FILE)
ifeq ($(ARCH),riscv64)
	make -C $(OPENSBI) -j$(nproc) PLATFORM=generic CROSS_COMPILE=$(TOOLCHAIN)- FW_PAYLOAD_PATH=$(IMAGE_FILE) \
	PLATFORM_RISCV_XLEN=64 PLATFORM_RISCV_ISA=rv64imac PLATFORM_RISCV_ABI=lp64 O=$(BUILD_DIR)
endif
