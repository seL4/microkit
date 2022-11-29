# seL4 Core Platform

The purpose of the seL4 Core Platform (sel4cp) is to enable system designers to create static software systems based on the seL4 microkernel.

The seL4 Core Platform consists of three parts:

   * seL4 Core Platform Library
   * seL4 Core Platform initial task
   * seL4 Core Platform tool

The seL4 Core Platform is distributed as a software development kit (SDK).

This repository is the source for the sel4cp SDK.

If you are *developing* sel4cp itself this is the repo you want!

If you are a system designer and want to *use* the sel4cp SDK please download a pre-built SDK.
Please see the manual in the SDK for instructions on using the SDK itself.

The remainder of this README is for sel4cp developers.

## Developer system requirements

Development of sel4cp has primarily been performed on Ubuntu 18.04 LTS (x86_64).

This section attempts to list the packages or external development tools which are required during development.
At this stage it may be incomplete.
Please file an issue if additional packages are required.

* git
* make
* python3.9
* python3.9-venv
* musl-1.2.2
* ARM GCC toolchain; 10.2-2020.11
* RISC-V GCC toolchain; 10.2.0-2020.12.8

On Ubuntu 18.04 there are no packages available for musl-1.2.2; it must be compiled from source.
On Ubuntu 18.04 Python 3.9 is available via the *deadsnakes* PPA: https://launchpad.net/~deadsnakes/+archive/ubuntu/ppa
To use this:

    $ sudo add-apt-repository ppa:deadsnakes/ppa
    $ sudo apt update
    $ sudo apt install python3.9 python3.9-venv

Additonally, a number of Python libraries are needed.
These should be installed using `pip`.

    $ python3.9 -m venv pyenv
    $ ./pyenv/bin/pip install --upgrade pip setuptools wheel
    $ ./pyenv/bin/pip install -r requirements.txt

Note: It is a high priority of the authors to ensure builds are self-contained and repeatable.
A high value is placed on using specifically versioned tools.
At this point in time this is not fully realised, however it is a high priority to enable this in the near future.

The ARM toolchain is available from:

https://developer.arm.com/tools-and-software/open-source-software/developer-tools/gnu-toolchain/gnu-a/downloads/10-2-2020-11

The specific version used for development is the x86_64-aarch64-none-elf version:

https://developer.arm.com/-/media/Files/downloads/gnu-a/10.2-2020.11/binrel/gcc-arm-10.2-2020.11-x86_64-aarch64-none-elf.tar.xz?revision=79f65c42-1a1b-43f2-acb7-a795c8427085&hash=61BBFB526E785D234C5D8718D9BA8E61

The RISC-V toolchain is available from:

https://github.com/sifive/freedom-tools/releases/tag/v2020.12.0

The specific version used for development is the x86_64-linux-ubuntu14 version:

https://static.dev.sifive.com/dev-tools/freedom-tools/v2020.12/riscv64-unknown-elf-toolchain-10.2.0-2020.12.8-x86_64-linux-ubuntu14.tar.gz

Note: There are no plans to support development of sel4cp on any platforms other than Linux x86_64.

## seL4 Version

The SDK includes a binary of the seL4 kernel.
During the SDK build process the kernel is build from source.

At this point in time there are some minor changes to the seL4 kernel required for seL4 Core Platform.

Please clone seL4 from:

    git@github.com:BreakawayConsulting/seL4.git

The correct branch to use is `sel4cp-core-support`.

Testing has been performed using commit `92f0f3ab28f00c97851512216c855f4180534a60`.

## Building the SDK

    $ ./pyenv/bin/python build_sdk.py --sel4=<path to sel4>

## Using the SDK

After building the SDK you probably want to build a system!
Please see the SDK user manual for documentation on the SDK itself.

When developing the SDK it is helpful to be able to build examples system quickly for testing purposes.
The `dev_build.py` script can be used for this purpose.
This script is not included in the SDK and is just meant for use of use of sel4cp developers.

By default `dev_build.py` will use the example source directly from the source directory.
In some cases you may want to test that the example source has been correctly included into the SDK.
To test this pass `--example-from-sdk` to the build script.

By default `dev_build.py` will use the the sel4cp tool directory from source (in `tool/sel4coreplat`).
However, in some cases it is desirable to test the sel4cp tool built into the SDK.
In this case pass `--tool-from-sdk` to use the tool that is built into the SDK.

Finally, by default the `dev_build.py` script relies on the default Makefile dependecy resolution.
However, in some cases it is useful to force a rebuild while doing SDK development.
For example, the `Makefile` can't know about the state of the sel4cp tool source code.
To support this a `--rebuild` option is provided.

## SDK Layout

The SDK is delivered as a `tar.gz` file.

The SDK top-level directory is `sel4cp-sdk-$VERSION`.

The directory layout underneath the top-level directory is:

```
bin/sel4cp
board/$board/$config/include/
board/$board/$config/include/sel4cp.h
board/$board/$config/lib/
board/$board/$config/lib/libsel4cp.a
board/$board/$config/lib/sel4cp.ld
board/$board/$config/elf
board/$board/$config/elf/loader.elf
board/$board/$config/elf/kernel.elf
board/$board/$config/elf/monitor.elf
doc/sel4cp_user_manual.pdf
```

The currently supported boards:

* tqma8xqp1gb

The currently supported configurations are:

* release
* debug

## Supported Boards

### tqma8xqp-1gb

The TQMa8Xx Embedded Module from TQ Group configured with the NXP i.MX8QXP SoC and 1GiB of DDR3 ECC memory.

https://www.tq-group.com/en/products/tq-embedded/arm-architecture/tqma8xx/

All testing has been performed with the module on the MBa8Xx carrier board which is included in the starter kit.

The provided board support should be at the module level and does not make any assumptions about the carrier board.

Note: There are different configured of the TQMa8Xx board which include different NXP SoCs and different memory configurations.
Such modules are not supported.

## Supported Configurations

## Release

In release configuration the loader, kernel and monitor do *not* perform any direct serial output.


## Debug

The debug configuration includes basic print output form the loader, kernel and monitor.
