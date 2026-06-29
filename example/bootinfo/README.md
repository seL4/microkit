<!--
     Copyright 2026, UNSW
     SPDX-License-Identifier: CC-BY-SA-4.0
-->
# Example - Hello World

This is a basic example that demonstrates how one can define
a MemoryRegion prefilled with BootInfo and map it to a PD, so
the PD can read the BootInfo in userland.

Supported BootInfo includes:
- x86_vbe
- x86_mbmap
- x86_acpi_rsdp
- x86_framebuffer
- x86_tsc_freq

As Microkit loader does not pass the DTB through on ARM and RISC-V,
so only x86 is supported in this example for now.

## Building

```sh
mkdir build
make BUILD_DIR=build MICROKIT_BOARD=<board> MICROKIT_CONFIG=<debug/release/benchmark> MICROKIT_SDK=/path/to/sdk qemu
```

## Running

See instructions for your board in the manual.
