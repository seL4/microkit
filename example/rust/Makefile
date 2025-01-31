#
# Copyright 2023, Colias Group, LLC
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

IMAGE_FILE = $(BUILD_DIR)/loader.img
REPORT_FILE = $(BUILD_DIR)/report.txt
SYSTEM_FILE := rust.system

all: $(IMAGE_FILE)

.PHONY: clean
clean:
	rm -rf $(BUILD_DIR)

$(BUILD_DIR):
	mkdir -p $@

BOARD_DIR := $(MICROKIT_SDK)/board/$(MICROKIT_BOARD)/$(MICROKIT_CONFIG)
SEL4_INCLUDE_DIRS := $(BOARD_DIR)/include
ARCH := ${shell grep 'CONFIG_SEL4_ARCH  ' $(BOARD_DIR)/include/kernel/gen_config.h | cut -d' ' -f4}

ifeq ($(ARCH),aarch64)
	RUST_TARGET := aarch64-sel4-microkit-minimal
else ifeq ($(ARCH),riscv64)
	RUST_TARGET := riscv64gc-sel4-microkit-minimal
else
$(error Unsupported ARCH)
endif

crate = $(BUILD_DIR)/$(1).elf

define build_crate

$(crate): $(crate).intermediate

.INTERMDIATE: $(crate).intermediate
$(crate).intermediate:
	SEL4_INCLUDE_DIRS=$(abspath $(SEL4_INCLUDE_DIRS)) \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			--target-dir $(BUILD_DIR)/target \
			--out-dir $(BUILD_DIR) \
			--target $(RUST_TARGET) \
			--release \
			-p $(1)

endef

CRATE_NAMES := hello

CRATES := $(foreach CRATE_NAME,$(CRATE_NAMES),$(call crate,$(CRATE_NAME)))

$(eval $(foreach CRATE_NAME,$(CRATE_NAMES),$(call build_crate,$(CRATE_NAME))))

$(IMAGE_FILE): $(SYSTEM_FILE) $(CRATES)
	$(MICROKIT_SDK)/bin/microkit \
		$< \
		--search-path $(BUILD_DIR) \
		--board $(MICROKIT_BOARD) \
		--config $(MICROKIT_CONFIG) \
		-r $(BUILD_DIR)/report.txt \
		-o $@
