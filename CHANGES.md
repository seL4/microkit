# Revision History for Microkit

## Release 2.0.0

This release contains various bug fixes, quality-of-life changes, features, and
new board support.

This is a major version bump due to a breaking change. Below the release notes,
there is a section on how to upgrade from Microkit 1.4.1.

### Features

* Add support for virtual machines with multiple vCPUs.
* Add support for ARM platforms to use SMC forwarding.
    * Note that this requires a kernel option to be set that right now is not
      set by default on any ARM platforms in Microkit.
* Allow tool to target Linux AArch64 hosts, also available as a pre-built SDK
  starting in this release.
* Increase max PD name length to 64 characters (from 16).
* Add Nix flake for Nix users who want to build from source and not depend on
  the pre-built SDK.
* Allow specifying whether a channel end can notify the other.
* Memory Regions now default to the largest possible page size for the MR's size
  and alignment of virtual addresses of its mappings.
    * This is transparent to users, but can lead to improved performance and
      memory usage.
* Utilise kernel API for naming threads in debug mode.
    * Helps debugging any kernel error prints in certain cases.
* Monitor fault handling now prints if a virtual memory exception was likely due
  to a PD stack overflow.
* Monitor fault handling now decodes user-exceptions likely caused by LLVM's
  UBSAN and prints appropriate debug output (only on AArch64).
* Add `setvar_size` attribute for MR mappings, similar to `setvar_vaddr`.
* Add a new 'Internals' section in the manual to document how the Microkit
  actually works.
* libmicrokit APIs for notifying, IRQ acking and performing PPCs now print
  errors when an invalid channel is used in debug mode.
    * Previously this lead to a kernel error print that was not that useful from
      within a Microkit environment.
* System images are now relocatable.
    * Previously they required loading at particular address within physical
      memory, the Microkit loader will now relocate itself if loaded at a
      different range of physical memory. This can make the boot-loading process
      of Microkit images less fragile.

### Bug fixes

* Allow mapping in 'null' page. Necessary for virtual machines with guest memory
  that starts at guest physical address zero.
* More error checking for invalid mappings (e.g virtual address outside of valid
  range).
* Re-add warning for unused memory regions.
    * This is a bug fix since it used to be part of the tool before it was rewritten.
* Actually enforce the PPC priority rule that PPC is only allowed to PDs of higher
  priority.
    * Note that to fix this in the tool, we had to introduce a breaking change
      in the SDF. See upgrade notes for details.
* Fix a bug with kernel boot emulation that would sometimes occur when using
  the release mode of the kernel.
* Fix the GIC device addresses the loader was using for QEMU during
  initialisation.
* Fixed a bug preventing allocating a MR at certain fixed physical addresses.
* Better error reporting when allocation of a memory region fails.
* Fix allocation of Scheduling Context objects, previously we were allocating
  more than necessary.

### Board support

* i.MX8MP-EVK
* Pine64 RockPro64
* Ariane (aka CVA6)
* Cheshire
* Raspberry Pi 4B

### Upgrade notes

There is a single breaking change in this release, it affects any
System Description Files (SDF) that make use of channels with PPCs.

This was done for two reasons:

1. It allows the tool to statically check and enforce the existing rule that a
   PD that performs a PPC must be lower priority that the PD it is calling into.
2. It allows finer granularity on the channel ends. For example, previously
   all channels allowed notifies either way, now that decision is up to the
   user. The default behaviour remains the same, both ends are allowed to
   notify each other.

To upgrade, follow these steps:

1. Remove the `pp` attribute from any PDs that have it.
2. Go to each channel with the PD that had the `pp` attribute. Set `pp="true"`
   on the end of the channel for the PD that is *performing* the PPC.

Below is an example of the upgrading a system that utilises PPCs.

From Microkit 1.4.1:
```xml
<protection_domain name="server" priority="254" pp="true">
    <program_image path="server.elf" />
</protection_domain>
<protection_domain name="client" priority="1">
    <program_image path="client.elf" />
</protection_domain>
<channel>
    <end pd="server" />
    <end pd="client" />
</channel>
```

To Microkit 2.0.0:
```xml
<protection_domain name="server" priority="254">
    <program_image path="server.elf" />
</protection_domain>
<protection_domain name="client" priority="1">
    <program_image path="client.elf" />
</protection_domain>
<channel>
    <end pd="server" />
    <end pd="client" pp="true" />
</channel>
```

Depending on your use-case you may also want to specify what end is allowed
to notify as well. For example, if the only communication between `server`
and `client` was PPC, you may want the channel to be the following:
```xml
<channel>
    <end pd="server" notify="false" />
    <end pd="client" notify="false" pp="true" />
</channel>
```

You can find all the details in the
[channel section of the manual](https://github.com/seL4/microkit/blob/2.0.0/docs/manual.md#channel).

## Release 1.4.1

This release contains various bug fixes. It does not include any new features.

* Fixed two bugs in the tool that lead to initialisation failure on larger Microkit systems.
* Disabled the `KernelArmVtimerUpdateVOffset` kernel configuration option by default.
  * This is necessary for Microkit VMs where they rely on knowing the actual surpassed time.
    More details are in the [pull request](https://github.com/seL4/microkit/pull/202).
* Enabled FPU for QEMU RISC-V virt and Pine64 Star64.
  * libmicrokit builds with hardware floating point enabled and, while it does not use the FPU,
    it means that every object linked with libmicrokit must also build with hardware floating
    point enabled. Previously using floating point operations would cause a crash in user-space.
* Fixed the loader link address for the MaaXBoard.
  * This does mean that if you target the MaaxBoard you will have to loader Microkit images at
    a different address. See the manual for details.
* Added error checking for overlapping memory regions.
* Included every TCB register in the monitor logs when a fault occurs.
* Made the tool compile from source with a Rust version lower that 1.79.0.
* Specified a minimum Rust version for the tool (1.73.0).
* Fixed typo in the `--help` output of the tool.
* Minor README fixes.
* Updated PyYAML dependency in requirements.txt to 6.0.2 (from 6.0).

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
