<!--
     Copyright 2021, Breakaway Consulting Pty. Ltd.
     SPDX-License-Identifier: CC-BY-SA-4.0
-->

# seL4 Microkit

The purpose of the seL4 Microkit is to enable system designers to create static software systems based on the seL4 microkernel.

The seL4 Microkit consists of five components:

   * Microkit bootloader
   * CapDL initialiser
   * Microkit library
   * Microkit monitor
   * Microkit tool

The Microkit is distributed as a software development kit (SDK).

This repository is the source for the Microkit SDK.

If you are a system designer and want to *use* the Microkit SDK please download a pre-built SDK from
[the latest release](https://github.com/seL4/microkit/releases).

If you need help getting started see the [seL4 documentation website](https://docs.sel4.systems/projects/microkit/)
as well as the manual in the SDK (`doc/manual.pdf`).

If you are *developing* Microkit itself see [DEVELOPER.md](./DEVELOPER.md).
