# Revision History for Microkit

## Release 1.3.0-dev

## Release 1.3.0

This release represents the first release since the seL4 Microkit was adopted by the
seL4 Foundation.

This release has no breaking changes.

### Features added

* Added support for 'passive' protection domains.
* Added protection domain hierarchy allowing PDs to manage faults caused by child PDs
  and control their execution.
* Added virtualisation support and a new 'virtual machine' abstraction that allows
  users to create systems with guest operating systems (such as Linux).
* Add the ability to specify the type of IRQ trigger on IRQ elements in the SDF. Previously
  all IRQs were registered as level triggered, now users are given the option of specifying
  an IRQ as 'edge' triggered which is needed for writing certain device drivers.
* Added support for building the Microkit SDK on macOS. If you are on macOS, you can now develop
  with Microkit without Docker or a virtual machine.

### Other changes

* Rewrote the Microkit tool from Python to Rust. This is meant to be a purely internal
  change and should not affect the use of the tool at all. This does however introduce
  a new dependency on Rust. See the README for building the new tool from source.
    * This was done primarily to decrease 3rd party dependencies and make it easier to build
      the Microkit SDK from source.

### Bug fixes

* Fixed the libmicrokit linker script to work with the LLVM linker, LLD. This means that non-GCC
  build systems can link with libmicrokit.
* Removed compiler provided includes (such as stdint.h and stdbool.h) from libmicrokit. This means
  that the libmicrokit header no longer depends on any system provided headers, making the SDK
  more self-contained.
* Various fixes and improvements to the manual.
* Various other bug-fixes and error message improvements to the Microkit tool.

### Board support

This release adds support for the following platforms:

* Avnet MaaXBoard
* HardKernel Odroid-C2
* HardKernel Odroid-C4
* NXP i.MX8MM-EVK
* NXP i.MX8MQ-EVK
* QEMU virt (AArch64)
* Xilinx ZCU102
