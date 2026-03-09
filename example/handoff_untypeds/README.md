<!--
     Copyright 2026, UNSW
     SPDX-License-Identifier: CC-BY-SA-4.0
-->
# Example - Handoff untypeds

This is a basic example that has a single protection domain
that receives capabilities to all remaining untyped memory,
prints information about the untypeds upon initialisation and
has an example of using these untypeds for creating new kernel objects.

All supported platforms are supported in this example.

## Building

```sh
mkdir build
make BUILD_DIR=build MICROKIT_BOARD=<board> MICROKIT_CONFIG=<debug/release/benchmark> MICROKIT_SDK=/path/to/sdk
```

## Running

See instructions for your board in the manual.
