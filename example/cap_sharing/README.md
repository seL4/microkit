<!--
     Copyright 2026, UNSW
     SPDX-License-Identifier: CC-BY-SA-4.0
-->
# Example - Cap Sharing

This is a basic example that demonstrates how one can use the `<cspace>` element
of a PD to give delegated access of specific capabilities associated with other
(or its own) protection domain.

See `cap_sharing.system` for some comments on the internal setup of this system.
The idea behind this example is that we have two PDs, one of which has control
over the TCB and scheduling context of the other; this could be the basis of
(e.g.) a user space scheduler.

All supported platforms are supported in this example.

## Building

```sh
mkdir build
make BUILD_DIR=build MICROKIT_BOARD=<board> MICROKIT_CONFIG=<debug/release/benchmark> MICROKIT_SDK=/path/to/sdk
```

## Running

See instructions for your board in the manual.

You should see the following output:

```
INFO  [sel4_capdl_initializer::initialize] Starting CapDL initializer
INFO  [sel4_capdl_initializer::initialize] Starting threads
MON|INFO: Microkit Monitor started!
|secondary| hello, world
|primary  | hello, world
|primary  | notifying secondary
|secondary| notified
|primary  | suspending secondary
|primary  | notifying secondary (it should not print)
|primary  | resuming secondary (it should then print)
|secondary| notified
|primary  | halting (success)...
```
