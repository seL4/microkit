<!--
     Copyright 2025, UNSW
     SPDX-License-Identifier: CC-BY-SA-4.0
-->
# Example - setvar

This is a basic example that demonstrate how to use some `setvar` functionalities that the Microkit provides.

Only QEMU virt (AArch64) is supported in this example. Though verything will work the same way on other
platforms. But on x86, `setvar region_paddr` won't be supported.

## Building

```sh
mkdir build
make BUILD_DIR=build MICROKIT_BOARD=qemu_virt_aarch64 MICROKIT_CONFIG=debug MICROKIT_SDK=/path/to/sdk
```

## Running

See instructions for your board in the manual.
