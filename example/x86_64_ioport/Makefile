#
# Copyright 2025, UNSW
#
# SPDX-License-Identifier: BSD-2-Clause
#
ifeq ($(strip $(BUILD_DIR)),)
$(error BUILD_DIR must be specified)
endif

ifeq ($(strip $(MICROKIT_SDK)),)
$(error MICROKIT_SDK must be specified)
endif

ifeq ($(strip $(MICROKIT_BOARD)),)
$(error MICROKIT_BOARD must be specified)
endif

ifeq ($(strip $(MICROKIT_CONFIG)),)
$(error MICROKIT_CONFIG must be specified)
endif

BOARD_DIR := $(MICROKIT_SDK)/board/$(MICROKIT_BOARD)/$(MICROKIT_CONFIG)

ARCH := ${shell grep 'CONFIG_SEL4_ARCH  ' $(BOARD_DIR)/include/kernel/gen_config.h | cut -d' ' -f4}

ifeq ($(ARCH),x86_64)
	TOOLCHAIN := x86_64-linux-gnu
	CFLAGS_ARCH := -march=x86-64 -mtune=generic
else
$(error Unsupported ARCH)
endif

CC := $(TOOLCHAIN)-gcc
LD := $(TOOLCHAIN)-ld
AS := $(TOOLCHAIN)-as
OBJCOPY := $(TOOLCHAIN)-objcopy
MICROKIT_TOOL ?= $(MICROKIT_SDK)/bin/microkit

IMAGES := x86_64_ioport.elf
CFLAGS := -nostdlib -ffreestanding -g3 -O3 -I$(BOARD_DIR)/include $(CFLAGS_ARCH)
LDFLAGS := -L$(BOARD_DIR)/lib -z noexecstack
LIBS := -lmicrokit -Tmicrokit.ld

INITIAL_TASK = $(BUILD_DIR)/capdl_initialiser_with_spec.elf
KERNEL = $(MICROKIT_SDK)/board/$(MICROKIT_BOARD)/$(MICROKIT_CONFIG)/elf/sel4.elf
COPIED_KERNEL = $(BUILD_DIR)/kernel.elf
REPORT = $(BUILD_DIR)/report.txt
SPEC = $(BUILD_DIR)/capdl_spec.json

all qemu: $(COPIED_KERNEL) $(INITIAL_TASK)

$(BUILD_DIR)/x86_64_ioport.o: x86_64_ioport.c Makefile
	$(CC) -c $(CFLAGS) -Wall -Wno-unused-function -Werror $< -o $@

$(BUILD_DIR)/x86_64_ioport.elf: $(BUILD_DIR)/x86_64_ioport.o
	$(LD) $(LDFLAGS) $^ $(LIBS) -o $@

$(INITIAL_TASK): $(addprefix $(BUILD_DIR)/, $(IMAGES)) x86_64_ioport.system
	$(MICROKIT_TOOL) x86_64_ioport.system --search-path $(BUILD_DIR) --board $(MICROKIT_BOARD) --config $(MICROKIT_CONFIG) -o $(INITIAL_TASK) -r $(REPORT) --capdl-spec $(SPEC)

# This step is required for 'qemu-system-x86_64' as the '-kernel' flag does not accept a 64-bit kernel.
$(COPIED_KERNEL): $(KERNEL)
	$(OBJCOPY) -O elf32-i386 $(KERNEL) $(COPIED_KERNEL)

qemu:
	qemu-system-x86_64                      \
			-cpu qemu64,+fsgsbase,+pdpe1gb,+xsaveopt,+xsave \
			-m "1G"                         \
			-display none                   \
			-serial mon:stdio               \
		-kernel $(COPIED_KERNEL) \
		-initrd $(INITIAL_TASK)
