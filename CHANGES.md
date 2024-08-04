# Revision History for Microkit

## Release 1.4.0-dev

## Release 1.4.0

This release aims to add support for requested features by the community, in order to
allow more people to use and transition to Microkit. There is of course still more to
be done over the next couple of releases.

This release has no breaking changes.

### Features added

* Added support for RISC-V 64-bit based platforms.
* Added a new 'benchmark' configuration to allow access to the in-kernel
  performance tracking.
* Add the ability to configure the stack size of a PD.
* Export ARM architectural timer to user-space for the QEMU virt AArch64 platform.
  * This platform does not have any other timer so this is allows having a timer
    driver when simulating/developing Microkit systems with QEMU.
* Add new APIs for 'deferred' versions of `microkit_notify` and `microkit_irq_ack`.
  See the manual for details on when and how to use these.

### Other changes

* Made a number of internal changes to the tool to improve performance and peak memory
  usage.
    * The tool's performance was not noticeable until building larger systems with Microkit.
      Now invoking the Microkit tool with a large system should not take more than 500ms-1s to
      complete. There are more opportunities for optimisation if we do run into the tool slowing
      down however.

### Bug fixes

* Fixed the loader to not print unless in debug mode (matching the behaviour of
  the kernel and monitor).
* Add error checking for duplicate symbols between `setvar_vaddr` attributes and
  `setvar` elements.
* Fixed an internal issue that prevented the Monitor from printing out correct fault
  information in debug mode.
* Fixed the parsing of parent protection domains, previously non-trivial cases were
  leading to errors.
* Fixed the tool to explicitly skip ELF segments that are not declared as loadable,
  previously the tool assumed all segments would be loaded at runtime.
* Fix permissions applied to the distributed/packaged SDK. Previously this would cause
  `sudo` access to move/remove the SDK.
* Fixed an internal issue that prevented a memory region from being allocated at a fixed
  physical address that is not part of device memory (e.g within RAM).

### Board support

This release adds support for the following platforms:

* QEMU virt (RISC-V 64-bit)
* Pine64 Star64

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
