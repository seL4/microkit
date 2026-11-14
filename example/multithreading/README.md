<!--
     Copyright 2026, UNSW
     SPDX-License-Identifier: CC-BY-SA-4.0
-->
# Example - Multithreading

This example demonstrates concurrent execution and shared-memory synchronization between two Protection Domains (PDs) in Microkit.

Both PDs (`pd_a` and `pd_b`) map a shared 4KB memory region containing a counter and an `atomic_flag` spinlock. Each PD increments the counter concurrently under the lock. Once both PDs complete their iterations, the final counter value is verified to ensure no updates were lost to race conditions.

All supported platforms are supported in this example.

## Building

```sh
mkdir build
make BUILD_DIR=build MICROKIT_BOARD=<board> MICROKIT_CONFIG=<debug/release/benchmark> MICROKIT_SDK=/path/to/sdk
```

## Running

See instructions for your board in the manual, or run with QEMU:

```sh
make BUILD_DIR=build MICROKIT_BOARD=<board> MICROKIT_CONFIG=<debug/release/benchmark> MICROKIT_SDK=/path/to/sdk qemu
```

You should see output similar to:

```
pd_a: starting
pd_b: starting
pd_a: finished increments
pd_b: finished increments
All PDs finished. Final counter: 20000 (expected: 20000)
SUCCESS
```
