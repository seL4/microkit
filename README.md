<!--
     Copyright 2021, Breakaway Consulting Pty. Ltd.
     SPDX-License-Identifier: CC-BY-SA-4.0
-->

# seL4 Microkit

The purpose of the seL4 Microkit is to enable system designers to create static software systems based on the seL4 microkernel.

The seL4 Microkit consists of three parts:

   * Microkit library
   * Microkit initial task
   * Microkit tool

The Microkit is distributed as a software development kit (SDK).

This repository is the source for the Microkit SDK.

The development of Microkit is on-going, more information can be found on the [roadmap](https://github.com/seL4/microkit/issues/61).

If you are *developing* Microkit itself this is the repo you want!

If you are a system designer and want to *use* the Microkit SDK please download a pre-built SDK.
Please see the manual in the SDK for instructions on using the SDK itself.

The remainder of this README is for Microkit developers.

## Developer system requirements

Building the Microkit SDK is supported on Linux (x86_64) and macOS (Apple Silicon/Intel).

This section attempts to list the packages or external development tools which are required during development.
At this stage it may be incomplete.
Please file an issue if additional packages are required.

* git
* make
* python3.9
* python3.9-venv
* musl-1.2.2 (only when targeting Linux)
* cmake
* ninja-build
* ARM GCC compiler for none-elf; version 12.2.1 20221205
* device tree compiler
* xmllint

To build the documentation you also need
* pandoc
* pdflatex
* texlive-latex-recommended
* texlive-fonts-recommended
* texlive-fonts-extra
* texlive-latex-extra

On a Debian-like system you can do:

    $ sudo apt install build-essential git cmake ninja-build \
	    device-tree-compiler libxml2-utils \
		pandoc texlive-latex-base texlive-latex-recommended \
		texlive-fonts-recommended texlive-fonts-extra \
		python3.9 python3.9-venv \
		musl-dev musl-tools

If you do not have Python 3.9 available, you can get it via the
*deadsnakes* PPA: https://launchpad.net/~deadsnakes/+archive/ubuntu/ppa
To use this:

    $ sudo add-apt-repository ppa:deadsnakes/ppa
    $ sudo apt update
    $ sudo apt install python3.9 python3.9-venv

On macOS, with the [Homebrew](https://brew.sh) package manager you can do:

    $ brew install pandoc cmake dtc ninja qemu libxml2 python@3.9 coreutils texlive

Additonally, a number of Python libraries are needed.
These should be installed using `pip`.

    $ python3.9 -m venv pyenv
    $ ./pyenv/bin/pip install --upgrade pip setuptools wheel
    $ ./pyenv/bin/pip install -r requirements.txt

Note: It is a high priority of the authors to ensure builds are self-contained and repeatable.
A high value is placed on using specifically versioned tools.
At this point in time this is not fully realised, however it is a high priority to enable this in the near future.

The ARM toolchain is available from:

https://developer.arm.com/downloads/-/arm-gnu-toolchain-downloads.

Development is done with the aarch64-none-elf- toolchain.

On Linux x86-64 the following version is used:
https://developer.arm.com/-/media/Files/downloads/gnu/12.2.rel1/binrel/arm-gnu-toolchain-12.2.rel1-x86_64-aarch64-none-elf.tar.xz?rev=28d5199f6db34e5980aae1062e5a6703&hash=F6F5604BC1A2BBAAEAC4F6E98D8DC35B

On macOS Apple Silicon/AArch64 the following version is used:
https://developer.arm.com/-/media/Files/downloads/gnu/12.2.rel1/binrel/arm-gnu-toolchain-12.2.rel1-darwin-arm64-aarch64-none-elf.tar.xz?rev=c5523a33dc7e49278f2a943a6a9822c4&hash=6DC6989BB1E6A9C7F8CBFEAA84842FA1

On macOS Intel/x86-64 the following version is used:
https://developer.arm.com/-/media/Files/downloads/gnu/12.2.rel1/binrel/arm-gnu-toolchain-12.2.rel1-darwin-x86_64-aarch64-none-elf.tar.xz?rev=09b11f159fc24fdda01e05bb32695dd5&hash=6AAF4239F28AE17389AB3E611DFFE0A6

## seL4 Version

The SDK includes a binary of the seL4 kernel.
During the SDK build process the kernel is build from source.

At this point in time there are some minor changes to the seL4 kernel required for Microkit. This is temporary, more details can be found [here](https://github.com/seL4/microkit/issues/52).

Please clone seL4 from:

    https://github.com/seL4/seL4.git

The correct branch to use is `microkit`.

Testing has been performed using commit `7008430d4432c71a74b2a1da0afae58f7a8658df`.

## Building the SDK

    $ ./pyenv/bin/python build_sdk.py --sel4=<path to sel4>

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
doc/
doc/microkit_user_manual.pdf
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
board/$board/$config/elf/kernel.elf
board/$board/$config/elf/monitor.elf
```

The currently supported boards are:

* imx8mm_evk
* imx8mq_evk
* odroidc2
* odroidc4
* tqma8xqp1gb
* zcu102

The currently supported configurations are:

* release
* debug

## Supported Boards

For documentation on each supported board see the [manual](https://github.com/seL4/microkit/blob/main/docs/manual.md#board-support-packages-bsps).

## Supported Configurations

## Release

In release configuration the loader, kernel and monitor do *not* perform any direct serial output.


## Debug

The debug configuration includes basic print output form the loader, kernel and monitor.
