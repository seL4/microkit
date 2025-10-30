<!--
     Copyright 2021, Breakaway Consulting Pty. Ltd.
     SPDX-License-Identifier: CC-BY-SA-4.0
-->

# Developing Microkit

Building the Microkit SDK is supported on Linux (x86_64) and macOS (Apple Silicon/Intel).

This section attempts to list the packages or external development tools which are required during development.

* Rust and Cargo
* git
* make
* python3.12
* python3.12-venv
* cmake
* ninja-build
* ARM GCC compiler for none-elf; version 12.2.Rel1
* RISC-V GCC compiler for unknown-elf; version 13.2.0
* device tree compiler
* xmllint
* qemu-system-aarch64
* qemu-system-riscv64
* qemu-system-x86_64

To build the documentation you also need
* pandoc
* pdflatex
* texlive-latex-recommended
* texlive-fonts-recommended
* texlive-fonts-extra
* texlive-latex-extra

### Linux (with apt)

On a Debian-like system you can do:

    $ curl https://sh.rustup.rs -sSf | sh
    $ rustup target add x86_64-unknown-linux-musl
    $ rustup component add rust-src --toolchain stable-x86_64-unknown-linux-gnu
    $ sudo apt install build-essential git cmake ninja-build \
        device-tree-compiler libxml2-utils \
        pandoc texlive-latex-base texlive-latex-recommended \
        texlive-fonts-recommended texlive-fonts-extra \
        python3.12 python3.12-venv \
        qemu-system-arm qemu-system-misc qemu-system-x86 \
        gcc-riscv64-unknown-elf \
        gcc-x86-64-linux-gnu
    $ python3.12 -m venv pyenv
    $ ./pyenv/bin/pip install --upgrade pip setuptools wheel
    $ ./pyenv/bin/pip install -r requirements.txt

If you do not have Python 3.12 available, you can get it via the
*deadsnakes* PPA: https://launchpad.net/~deadsnakes/+archive/ubuntu/ppa
To use this:

    $ sudo add-apt-repository ppa:deadsnakes/ppa
    $ sudo apt update
    $ sudo apt install python3.12 python3.12-venv

The ARM toolchain is available from:

https://developer.arm.com/downloads/-/arm-gnu-toolchain-downloads.

Development is done with the aarch64-none-elf- toolchain.

On Linux x86-64 the following version is used:
https://developer.arm.com/-/media/Files/downloads/gnu/12.2.rel1/binrel/arm-gnu-toolchain-12.2.rel1-x86_64-aarch64-none-elf.tar.xz?rev=28d5199f6db34e5980aae1062e5a6703&hash=F6F5604BC1A2BBAAEAC4F6E98D8DC35B

### macOS

On macOS, with the [Homebrew](https://brew.sh) package manager you can do:

    $ curl https://sh.rustup.rs -sSf | sh
    $ brew tap riscv-software-src/riscv
    $ brew install riscv-tools
    $ brew install x86_64-elf-gcc pandoc cmake dtc ninja libxml2 python@3.12 coreutils texlive qemu
    $ python3.12 -m venv pyenv
    $ ./pyenv/bin/pip install --upgrade pip setuptools wheel
    $ ./pyenv/bin/pip install -r requirements.txt

The ARM toolchain is available from:

https://developer.arm.com/downloads/-/arm-gnu-toolchain-downloads.

Development is done with the aarch64-none-elf- toolchain.

On macOS Apple Silicon/AArch64 the following version is used:
https://developer.arm.com/-/media/Files/downloads/gnu/12.2.rel1/binrel/arm-gnu-toolchain-12.2.rel1-darwin-arm64-aarch64-none-elf.tar.xz?rev=c5523a33dc7e49278f2a943a6a9822c4&hash=6DC6989BB1E6A9C7F8CBFEAA84842FA1

On macOS Intel/x86-64 the following version is used:
https://developer.arm.com/-/media/Files/downloads/gnu/12.2.rel1/binrel/arm-gnu-toolchain-12.2.rel1-darwin-x86_64-aarch64-none-elf.tar.xz?rev=09b11f159fc24fdda01e05bb32695dd5&hash=6AAF4239F28AE17389AB3E611DFFE0A6

### Nix

Running:

    $ nix develop

Will give a shell with all the required dependencies to build the SDK.

An important note is that Nix's RISC-V cross-compiler will have a different
prefix to the default one the SDK build script expects.

When you build the SDK, provide two extra arguments:
`--gcc-toolchain-prefix-x86_64 x86_64-elf --gcc-toolchain-prefix-riscv64 riscv64-none-elf`.

## seL4 Version

The SDK includes a binary of the seL4 kernel.
During the SDK build process the kernel is build from source.

Please clone seL4 from:

    https://github.com/seL4/seL4.git

The correct branch to use is `microkit`.

Testing has been performed using commit `3aafe9e0b9527794c547d12090117e1000302da0`.

## Building the SDK

    $ ./pyenv/bin/python build_sdk.py --sel4=<path to sel4>

The SDK will be in `release/`.

See the help menu of `build_sdk.py` for configuring how the SDK is built:

    $ ./pyenv/bin/python build_sdk.py --help

## Using the SDK

After building the SDK you probably want to build a system!
Please see the SDK user manual for documentation on the SDK itself.

When developing the SDK it is helpful to be able to build examples system quickly for testing purposes.
The `dev_build.py` script can be used for this purpose.
This script is not included in the SDK and is just meant for use of use of Microkit developers.

By default `dev_build.py` will use the example source directly from the source directory.
In some cases you may want to test that the example source has been correctly included into the SDK.
To test this pass `--example-from-sdk` to the build script.

By default `dev_build.py` will use the the Microkit tool directory from source (in `tool/microkit`).
However, in some cases it is desirable to test the Microkit tool built into the SDK.
In this case pass `--tool-from-sdk` to use the tool that is built into the SDK.

Finally, by default the `dev_build.py` script relies on the default Makefile dependecy resolution.
However, in some cases it is useful to force a rebuild while doing SDK development.
For example, the `Makefile` can't know about the state of the Microkit tool source code.
To support this a `--rebuild` option is provided.

## SDK Layout

The SDK is delivered as a `tar.gz` file.

The SDK top-level directory is `microkit-sdk-$VERSION`.

The directory layout underneath the top-level directory is:

```
VERSION
LICENSE.md
LICENSES/$licence.txt
doc/
doc/microkit_user_manual.pdf
example/
example/$example/
bin/
bin/microkit
board/
board/$board/$config/include/
board/$board/$config/include/microkit.h
board/$board/$config/lib/
board/$board/$config/lib/libmicrokit.a
board/$board/$config/lib/microkit.ld
board/$board/$config/elf/
board/$board/$config/elf/loader.elf
board/$board/$config/elf/sel4.elf
board/$board/$config/elf/monitor.elf
```

### Supported Boards

For documentation on each supported board see the [manual](https://github.com/seL4/microkit/blob/main/docs/manual.md#board-support-packages-bsps).

### Supported Configurations

For documentation on each supported board see the [manual](https://github.com/seL4/microkit/blob/main/docs/manual.md#configurations-config).
