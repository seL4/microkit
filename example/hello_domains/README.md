<!--
     Copyright 2024, UNSW
     SPDX-License-Identifier: CC-BY-SA-4.0
-->
# Example - Hello World

This is a basic hello world example with domains. There are three protection
domains, all with the same source ELF. Each protection domain is assigned to
its own domain. The domain schedule will infinitely cycle through these domains.

All supported platforms are supported in this example.

## Building

```sh
mkdir build
make BUILD_DIR=build MICROKIT_BOARD=<board> MICROKIT_CONFIG=<debug/release/benchmark> MICROKIT_SDK=/path/to/sdk
```

## Running

See instructions for your board in the manual.
