#!/usr/bin/env bash

# Copyright 2026, UNSW
# SPDX-License-Identifier: BSD-2-Clause

set -e

if [ -z "${NO_APT_UPDATE}" ]; then
    sudo apt-get update
fi

march="$1"
shift

do_aarch64() {
    wget -O aarch64-toolchain.tar.gz https://sel4-toolchains.s3.us-east-2.amazonaws.com/arm-gnu-toolchain-12.2.rel1-x86_64-aarch64-none-elf.tar.xz%3Frev%3D28d5199f6db34e5980aae1062e5a6703%26hash%3DF6F5604BC1A2BBAAEAC4F6E98D8DC35B
    tar xf aarch64-toolchain.tar.gz
    echo "$(pwd)/arm-gnu-toolchain-12.2.rel1-x86_64-aarch64-none-elf/bin" >> $GITHUB_PATH
}

do_riscv64() {
    sudo apt-get install -qq gcc-riscv64-unknown-elf
}

do_x86_64() {
    sudo apt-get install -qq gcc-x86-64-linux-gnu
}

case "${march}" in
    aarch64)
        do_aarch64
        ;;

    riscv64)
        do_riscv64
        ;;

    x86_64)
        do_x86_64
        ;;

    *)
        echo "Unknown or empty march value '${march}'" >&2
        exit 1
        ;;
esac
