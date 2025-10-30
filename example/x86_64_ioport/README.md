<!--
     Copyright 2025, UNSW
     SPDX-License-Identifier: CC-BY-SA-4.0
-->
# Example - Hello World

This is a basic hello world example that has a single protection domain
that simply prints "hello!" via the first serial I/O port (0x3F8) upon initialisation.

Only x86_64 platforms are supported.

## Building

```sh
mkdir build
make BUILD_DIR=build MICROKIT_BOARD=<board> MICROKIT_CONFIG=<debug/release/benchmark> MICROKIT_SDK=/path/to/sdk
```

## Running

See instructions for your board in the manual.
