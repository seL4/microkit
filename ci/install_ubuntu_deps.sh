#!/usr/bin/env bash

# Copyright 2026, UNSW
# SPDX-License-Identifier: BSD-2-Clause

set -e

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

rustup install 1.94.0
rustup default 1.94.0
rustup target add x86_64-unknown-linux-musl
rustup component add rust-src --toolchain 1.94.0-x86_64-unknown-linux-gnu
rustup target add aarch64-unknown-none
rustup target add riscv64gc-unknown-none-elf
rustup target add x86_64-unknown-none

sudo apt-get update

NO_APT_UPDATE=1 $SCRIPT_DIR/install_march_build_deps.sh aarch64
NO_APT_UPDATE=1 $SCRIPT_DIR/install_march_build_deps.sh riscv64
NO_APT_UPDATE=1 $SCRIPT_DIR/install_march_build_deps.sh x86_64

# sel4-only dependencies
sudo apt-get install -qq software-properties-common
sudo add-apt-repository ppa:deadsnakes/ppa
sudo apt-get install -qq \
    cmake pandoc device-tree-compiler ninja-build \
    texlive-latex-base texlive-latex-recommended \
    texlive-fonts-recommended texlive-fonts-extra \
    libxml2-utils \
    python3.12 python3-pip python3.12-venv \
    qemu-system-arm qemu-system-misc

python3.12 -m venv pyenv
./pyenv/bin/pip install --upgrade pip setuptools wheel
./pyenv/bin/pip install -r requirements.txt
