source ~/system/myvenv/bin/activate
BOARDS=x86_64_generic
python build_sdk.py --skip-docs --skip-tar --configs=debug --boards=$BOARDS --sel4=/home/cheng/work/seL4
cd ./example/hello
make MICROKIT_SDK=/home/cheng/work/microkit/release/microkit-sdk-2.1.0-dev MICROKIT_CONFIG=debug BUILD_DIR=build MICROKIT_BOARD=x86_64_generic

qemu-system-x86_64 -machine q35 -kernel /home/cheng/work/microkit/release/microkit-sdk-2.1.0-dev/board/x86_64_generic/debug/elf/sel4_32.elf -m size=2G -serial mon:stdio -cpu qemu64,+fsgsbase,+pdpe1gb,+pcid,+invpcid,+xsave,+xsaves,+xsaveopt -initrd loader.img -device intel-iommu,intremap=on,caching-mode=on -device virtio-blk-pci,drive=hd,addr=0x3.0,iommu_platform=on,disable-legacy=on \
-nographic \
-d guest_errors \
-drive file=disk,if=none,format=raw,id=hd

/home/cheng/work/microkit/release/microkit-sdk-2.1.0-dev/bin/microkit blk.system --search-path /home/cheng/work/sddf/examples/blk/build --board x86_64_generic --config debug -o loader.img -r report.txt

qemu-system-x86_64 -machine q35 -kernel /home/cheng/work/microkit/release/microkit-sdk-2.1.0-dev/board/x86_64_generic/debug/elf/sel4_32.elf -m size=2G -serial mon:stdio -cpu qemu64,+fsgsbase,+pdpe1gb,+pcid,+invpcid,+xsave,+xsaves,+xsaveopt -initrd loader.img -device intel-iommu,intremap=on,caching-mode=on -device virtio-blk-pci,drive=hd,addr=0x3.0,iommu_platform=on,disable-legacy=on     -nographic     -d guest_errors     -drive file=disk,if=none,format=raw,id=hd