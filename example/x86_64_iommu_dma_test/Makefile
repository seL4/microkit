#
# Copyright 2026, UNSW
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

IMAGES := x86_64_iommu_dma_test.elf
CFLAGS := -nostdlib -ffreestanding -g3 -O3 -I$(BOARD_DIR)/include $(CFLAGS_ARCH)
LDFLAGS := -L$(BOARD_DIR)/lib -z noexecstack
LIBS := -lmicrokit -Tmicrokit.ld

INITIAL_TASK = $(BUILD_DIR)/initialiser.elf
KERNEL = $(BUILD_DIR)/sel4.elf
KERNEL_32B = $(BUILD_DIR)/sel4_32.elf
REPORT = $(BUILD_DIR)/report.txt
SPEC = $(BUILD_DIR)/capdl_spec.json

all: $(KERNEL) $(INITIAL_TASK)
qemu: $(KERNEL_32B) $(INITIAL_TASK)

$(BUILD_DIR)/x86_64_iommu_dma_test.o: x86_64_iommu_dma_test.c Makefile
	$(CC) -c $(CFLAGS) $< -o $@

$(BUILD_DIR)/x86_64_iommu_dma_test.elf: $(BUILD_DIR)/x86_64_iommu_dma_test.o
	$(LD) $(LDFLAGS) $^ $(LIBS) -o $@

$(KERNEL) $(KERNEL_32B) $(INITIAL_TASK): $(addprefix $(BUILD_DIR)/, $(IMAGES)) x86_64_iommu_dma_test.system
	$(MICROKIT_TOOL) x86_64_iommu_dma_test.system --search-path $(BUILD_DIR) --board $(MICROKIT_BOARD) --config $(MICROKIT_CONFIG) -o $(INITIAL_TASK) -r $(REPORT) --capdl-json $(SPEC)

qemu:
	qemu-system-x86_64                                      				\
			-machine q35                                  					\
			-cpu qemu64,+fsgsbase,+pdpe1gb,+xsaveopt,+xsave 				\
			-m "4G"                                      					\
			-display none                                					\
			-serial mon:stdio                            					\
			-device intel-iommu,intremap=off,aw-bits=39      				\
			-device edu,dma_mask=0xffffffffffffffff,bus=pcie.0,addr=0x04.0  \
		-kernel $(KERNEL_32B)                             					\
		-initrd $(INITIAL_TASK)
