#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#

ifneq ($(strip $(RUST_ONLY)),)
	ARCH := dummy
	BOARD := dummy
	LINK_ADDRESS := dummy
	TARGET_TRIPLE := dummy
	SEL4_SDK := dummy
endif

ifeq ($(strip $(BUILD_DIR)),)
$(error BUILD_DIR must be specified)
endif

ifeq ($(strip $(ARCH)),)
$(error ARCH must be specified)
endif

ifeq ($(strip $(BOARD)),)
$(error BOARD must be specified)
endif

ifeq ($(strip $(LINK_ADDRESS)),)
$(error LINK_ADDRESS must be specified)
endif

ifeq ($(strip $(TARGET_TRIPLE)),)
$(error TARGET_TRIPLE must be specified)
endif

ifeq ($(strip $(LLVM)),True)
  CC = clang -target $(TARGET_TRIPLE)
  CPP = clang-cpp -target $(TARGET_TRIPLE)
  AS = clang -target $(TARGET_TRIPLE)
  LD = ld.lld
else
  CC = $(TARGET_TRIPLE)-gcc
  CPP = $(TARGET_TRIPLE)-cpp
  AS = $(TARGET_TRIPLE)-as
  LD = $(TARGET_TRIPLE)-ld
endif

RUSTC := rustc
CLIPPY := clippy-driver
RUSTFMT := rustfmt

ifeq ($(ARCH),aarch64)
	CFLAGS_AARCH64 := -mcpu=$(GCC_CPU) -mgeneral-regs-only -mstrict-align -mno-outline-atomics
	CFLAGS_ARCH := $(CFLAGS_AARCH64) -DARCH_aarch64
	ASM_FLAGS_ARCH := -mcpu=$(GCC_CPU)
	ARCH_DIR := aarch64
	RUST_TARGET_TRIPLE := aarch64-unknown-none
else ifeq ($(ARCH),riscv64)
	CFLAGS_RISCV64 := -mcmodel=medany -march=rv64gc -mabi=lp64d
	CFLAGS_ARCH := $(CFLAGS_RISCV64) -DARCH_riscv64
	ASM_FLAGS_ARCH := -march=rv64gc -mabi=lp64d
	RUST_TARGET_TRIPLE := riscv64gc-unknown-none-elf
	ARCH_DIR := riscv
endif

CFLAGS := -std=gnu11 -g -O3 -nostdlib -ffreestanding \
					-MP -MD $(CFLAGS_ARCH) -DBOARD_$(BOARD) -I$(SEL4_SDK)/include \
					-Wall -Werror -Wstrict-prototypes -Wmissing-prototypes -Wmissing-declarations \
					-Wundef -Wno-nonnull -Wnested-externs -ffunction-sections -fdata-sections

ASM_FLAGS := $(ASM_FLAGS_ARCH) -g -MP -MD -I$(SEL4_SDK)/include

RUST_EDITION := 2024
RUSTFLAGS := --edition $(RUST_EDITION) -g -C opt-level=2
CLIPPY_FLAGS ?=

PROGS := loader.elf
OBJECTS := loader.o crt0.o uart.o cutil.o libpage_tables.a

ifeq ($(ARCH),aarch64)
	OBJECTS += util64.o el.o exceptions.o init.o mmu.o cpus.o
else ifeq ($(ARCH),riscv64)
	OBJECTS += exceptions.o init.o mmu.o cpus.o sbi.o
endif

RUST_CRATES := page_tables

LINKSCRIPT_INPUT := $(ARCH).ld
LINKSCRIPT := $(BUILD_DIR)/link.ld

.SECONDARY:

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.S
	$(CC) -DLINK_ADDRESS=$(LINK_ADDRESS) -x assembler-with-cpp -c $(ASM_FLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.s
	$(AS) -c $< -o $@

$(BUILD_DIR)/%.o : src/$(ARCH_DIR)/%.c
	$(CC) -c $(CFLAGS) $< -o $@

$(BUILD_DIR)/%.o : src/%.c
	$(CC) -c $(CFLAGS) $< -o $@

# Note: having multiple libs with staticlib will give duplicate linker symbol
# issues. Use "--crate-type rlib" instead, but then we need to link a single
# copy of the rust corelibs. For now this is fine.
$(BUILD_DIR)/lib%.a : src/%.rs
	$(RUSTC) $(RUSTFLAGS) \
		--emit dep-info,metadata,link \
		--out-dir $(BUILD_DIR) -L dependency=$(BUILD_DIR) \
		--target $(RUST_TARGET_TRIPLE) \
		--crate-type staticlib \
		--crate-name $(patsubst lib%.a,%,$(notdir $@)) \
		$<

-include $(BUILD_DIR)/*.d

OBJPROG = $(addprefix $(BUILD_DIR)/, $(PROGS))

all: $(OBJPROG)

$(LINKSCRIPT): $(LINKSCRIPT_INPUT)
	$(CPP) -DLINK_ADDRESS=$(LINK_ADDRESS) $< | grep -v "^#" > $@

LDFLAGS := -T$(LINKSCRIPT)

$(OBJPROG): $(addprefix $(BUILD_DIR)/, $(OBJECTS)) $(LINKSCRIPT)
	$(LD) $(LDFLAGS) --start-group $(addprefix $(BUILD_DIR)/, $(OBJECTS)) --end-group -o $@

rusttest_%: src/%.rs
	$(RUSTC) $(RUSTFLAGS) \
		--emit dep-info,metadata,link \
		--out-dir $(BUILD_DIR) -L dependency=$(BUILD_DIR) \
		-Awarnings \
		--test \
		--crate-name "$@" \
		$<

tests: $(addprefix rusttest_, $(RUST_CRATES))
	$(BUILD_DIR)/rusttest_page_tables

rustclippy_%: src/%.rs
	$(CLIPPY) $(RUSTFLAGS) \
		--emit dep-info,metadata,link \
		--out-dir $(BUILD_DIR) -L dependency=$(BUILD_DIR) \
		$(CLIPPYFLAGS) \
		-Cpanic=abort \
		--crate-type staticlib \
		--crate-name "$@" \
		$<

clippy: $(addprefix rustclippy_, $(RUST_CRATES))

rustfmt_%: src/%.rs
	$(RUSTFMT) $(RUSTFMT_FLAGS) \
		--edition $(RUST_EDITION) \
		$<

rustfmt: $(addprefix rustfmt_, $(RUST_CRATES))

rustfmt-check: RUSTFMT_FLAGS += --check
rustfmt-check: rustfmt
