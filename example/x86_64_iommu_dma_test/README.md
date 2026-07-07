<!--
     Copyright 2026, UNSW
     SPDX-License-Identifier: CC-BY-SA-4.0
-->
# x86_64_iommu_dma_test

This example checks x86 IOMMU mappings with QEMU's EDU PCI device. It maps one
RAM page into the EDU device's IOSpace, asks the device to DMA from that page
into its internal SRAM, then asks the device to DMA the SRAM contents back into
a second offset in the same RAM page and verifies the data.

The QEMU command in the Makefile pins the EDU device at PCI BDF `00:04.0`, which
must match the `pcidev="0:4.0"` attribute in the system description.

To build and run:

```sh
mkdir build
make -C example/x86_64_iommu_dma_test \
    BUILD_DIR=build \
    MICROKIT_SDK=/path/to/microkit-sdk \
    MICROKIT_BOARD=x86_64_generic \
    MICROKIT_CONFIG=debug \
    qemu
```
