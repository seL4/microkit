<!--
     Copyright 2025, UNSW
     SPDX-License-Identifier: CC-BY-SA-4.0
-->
# Example - Rust

This example is the same as the 'hello' example expect that the
protection domain is written in Rust instead of C. This makes
use of the [rust-sel4](https://github.com/seL4/rust-sel4) Microkit
support.

You can find more complicated example systems written in Rust
at the following links:

* https://github.com/seL4/rust-microkit-demo
* https://github.com/seL4/rust-microkit-http-server-demo

All supported platforms are supported in this example.

## Building

```sh
mkdir build
make BUILD_DIR=build MICROKIT_BOARD=<board> MICROKIT_CONFIG=<debug/release/benchmark> MICROKIT_SDK=/path/to/sdk
```

## Running

See instructions for your board in the manual.
