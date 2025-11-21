# Copyright 2021, Breakaway Consulting Pty. Ltd.
# SPDX-License-Identifier: BSD-2-Clause

"""The SDK build script.

# Why Python (and not make, or something else)?

We call out to Make, but having this top-level driver script
is useful.

There are just a lot of things that are much easier in Python
than in make.

"""
from argparse import ArgumentParser
import copy
from os import popen, system, environ
import shutil
from pathlib import Path
from dataclasses import dataclass
from sys import executable
from tarfile import open as tar_open, TarInfo
import platform as host_platform
from enum import IntEnum
import json

from typing import Any, Dict, Union, List, Tuple, Optional

NAME = "microkit"

ENV_BIN_DIR = Path(executable).parent

MICROKIT_EPOCH = 1616367257

TRIPLE_AARCH64 = "aarch64-none-elf"
TRIPLE_RISCV = "riscv64-unknown-elf"
# TODO: this won't work for LLVM, to fix later
TRIPLE_X86_64 = "x86_64-linux-gnu"

KERNEL_CONFIG_TYPE = Union[bool, str]
KERNEL_OPTIONS = Dict[str, Union[bool, str]]

DEFAULT_KERNEL_OPTIONS = {
    "KernelIsMCS": True,
    "KernelRootCNodeSizeBits": "17",
}

DEFAULT_KERNEL_OPTIONS_AARCH64 = {
    "KernelArmExportPCNTUser": True,
    "KernelArmHypervisorSupport": True,
    "KernelArmVtimerUpdateVOffset": False,
    "KernelAllowSMCCalls": True,
} | DEFAULT_KERNEL_OPTIONS

DEFAULT_KERNEL_OPTIONS_RISCV64 = DEFAULT_KERNEL_OPTIONS

DEFAULT_KERNEL_OPTIONS_X86_64 = {
    "KernelPlatform": "pc99",
    "KernelX86MicroArch": "generic",
} | DEFAULT_KERNEL_OPTIONS


class KernelArch(IntEnum):
    AARCH64 = 1
    RISCV64 = 2
    X86_64 = 3

    def target_triple(self) -> str:
        if self == KernelArch.AARCH64:
            return TRIPLE_AARCH64
        elif self == KernelArch.RISCV64:
            return TRIPLE_RISCV
        elif self == KernelArch.X86_64:
            return TRIPLE_X86_64
        else:
            raise Exception(f"Unsupported toolchain architecture '{self}'")

    def rust_toolchain(self) -> str:
        if self == KernelArch.AARCH64:
            return f"{self.to_str()}-sel4-minimal"
        elif self == KernelArch.RISCV64:
            return f"{self.to_str()}imac-sel4-minimal"
        elif self == KernelArch.X86_64:
            return f"{self.to_str()}-sel4-minimal"
        else:
            raise Exception(f"Unsupported toolchain target triple '{self}'")

    def is_riscv(self) -> bool:
        return self == KernelArch.RISCV64

    def is_arm(self) -> bool:
        return self == KernelArch.AARCH64

    def is_x86(self) -> bool:
        return self == KernelArch.X86_64

    def to_str(self) -> str:
        if self == KernelArch.AARCH64:
            return "aarch64"
        elif self == KernelArch.RISCV64:
            return "riscv64"
        elif self == KernelArch.X86_64:
            return "x86_64"
        else:
            raise Exception(f"Unsupported arch {self}")

    def as_kernel_arch_config(self) -> tuple[str, str]:
        return ("KernelSel4Arch", self.to_str())


KERNEL_OPTIONS_ARCH = Dict[KernelArch, KERNEL_OPTIONS]


@dataclass
class BoardInfo:
    name: str
    arch: KernelArch
    gcc_cpu: Optional[str]
    loader_link_address: Optional[int]
    kernel_options: KERNEL_OPTIONS
    smp_cores: Optional[int] = None


@dataclass
class ConfigInfo:
    name: str
    debug: bool
    kernel_options: KERNEL_OPTIONS
    kernel_options_arch: KERNEL_OPTIONS_ARCH


SUPPORTED_BOARDS = (
    BoardInfo(
        name="tqma8xqp1gb",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a35",
        loader_link_address=0x80280000,
        kernel_options={
            "KernelPlatform": "tqma8xqp1gb",
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="zcu102",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a53",
        loader_link_address=0x40000000,
        kernel_options={
            "KernelPlatform": "zynqmp",
            "KernelARMPlatform": "zcu102",
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="maaxboard",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a53",
        loader_link_address=0x50000000,
        kernel_options={
            "KernelPlatform": "maaxboard",
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
        smp_cores=4,
    ),
    BoardInfo(
        name="imx8mm_evk",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a53",
        loader_link_address=0x41000000,
        kernel_options={
            "KernelPlatform": "imx8mm-evk",
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="imx8mp_evk",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a53",
        loader_link_address=0x41000000,
        kernel_options={
            "KernelPlatform": "imx8mp-evk",
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="imx8mq_evk",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a53",
        loader_link_address=0x41000000,
        kernel_options={
            "KernelPlatform": "imx8mq-evk",
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="imx8mp_iotgate",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a53",
        loader_link_address=0x50000000,
        kernel_options={
            "KernelPlatform": "imx8mp-evk",
            "KernelCustomDTS": "custom_dts/iot-gate.dts",
            "KernelCustomDTSOverlay": "src/plat/imx8m-evk/overlay-imx8mp-evk.dts",
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="odroidc2",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a53",
        loader_link_address=0x20000000,
        kernel_options={
            "KernelPlatform": "odroidc2",
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="odroidc4",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a55",
        loader_link_address=0x20000000,
        smp_cores=4,
        kernel_options={
            "KernelPlatform": "odroidc4",
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="ultra96v2",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a53",
        loader_link_address=0x40000000,
        kernel_options={
            "KernelPlatform": "zynqmp",
            "KernelARMPlatform": "ultra96v2",
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="qemu_virt_aarch64",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a53",
        loader_link_address=0x70000000,
        smp_cores=4,
        kernel_options={
            "KernelPlatform": "qemu-arm-virt",
            "QEMU_MEMORY": "2048",
            # There is no peripheral timer, so we use the ARM
            # architectural timer
            "KernelArmExportPTMRUser": True,
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="qemu_virt_riscv64",
        arch=KernelArch.RISCV64,
        gcc_cpu=None,
        loader_link_address=0x90000000,
        smp_cores=4,
        kernel_options={
            "KernelPlatform": "qemu-riscv-virt",
            "QEMU_MEMORY": "2048",
        } | DEFAULT_KERNEL_OPTIONS_RISCV64,
    ),
    BoardInfo(
        name="rpi4b_1gb",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a72",
        loader_link_address=0x10000000,
        kernel_options={
            "KernelPlatform": "bcm2711",
            "RPI4_MEMORY": 1024,
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="rpi4b_2gb",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a72",
        loader_link_address=0x10000000,
        kernel_options={
            "KernelPlatform": "bcm2711",
            "RPI4_MEMORY": 2048,
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="rpi4b_4gb",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a72",
        loader_link_address=0x10000000,
        kernel_options={
            "KernelPlatform": "bcm2711",
            "RPI4_MEMORY": 4096,
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="rpi4b_8gb",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a72",
        loader_link_address=0x10000000,
        kernel_options={
            "KernelPlatform": "bcm2711",
            "RPI4_MEMORY": 8192,
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="rockpro64",
        arch=KernelArch.AARCH64,
        gcc_cpu="cortex-a53",
        loader_link_address=0x30000000,
        kernel_options={
            "KernelPlatform": "rockpro64",
        } | DEFAULT_KERNEL_OPTIONS_AARCH64,
    ),
    BoardInfo(
        name="hifive_p550",
        arch=KernelArch.RISCV64,
        gcc_cpu=None,
        loader_link_address=0x90000000,
        smp_cores=4,
        kernel_options={
            "KernelPlatform": "hifive-p550",
        } | DEFAULT_KERNEL_OPTIONS_RISCV64,
    ),
    BoardInfo(
        name="star64",
        arch=KernelArch.RISCV64,
        gcc_cpu=None,
        loader_link_address=0x60000000,
        smp_cores=4,
        kernel_options={
            "KernelPlatform": "star64",
        } | DEFAULT_KERNEL_OPTIONS_RISCV64,
    ),
    BoardInfo(
        name="ariane",
        arch=KernelArch.RISCV64,
        gcc_cpu=None,
        loader_link_address=0x90000000,
        kernel_options={
            "KernelPlatform": "ariane",
        } | DEFAULT_KERNEL_OPTIONS_RISCV64,
    ),
    BoardInfo(
        name="cheshire",
        arch=KernelArch.RISCV64,
        gcc_cpu=None,
        loader_link_address=0x90000000,
        kernel_options={
            "KernelPlatform": "cheshire",
        } | DEFAULT_KERNEL_OPTIONS_RISCV64,
    ),
    BoardInfo(
        name="serengeti",
        arch=KernelArch.RISCV64,
        gcc_cpu=None,
        loader_link_address=0x90000000,
        kernel_options={
            "KernelPlatform": "cheshire",
        } | DEFAULT_KERNEL_OPTIONS_RISCV64,
    ),
    BoardInfo(
        name="x86_64_generic",
        arch=KernelArch.X86_64,
        gcc_cpu="generic",
        loader_link_address=None,
        kernel_options={
            # @billn revisit
            "KernelSupportPCID": False,
            "KernelVTX": False,
        } | DEFAULT_KERNEL_OPTIONS_X86_64,
    ),
    # This particular configuration requires support for Intel VT-x
    # (plus nested virtualisation on your host if targeting QEMU).
    # AMD SVM is currently unsupported by seL4.
    BoardInfo(
        name="x86_64_generic_vtx",
        arch=KernelArch.X86_64,
        gcc_cpu="generic",
        loader_link_address=None,
        kernel_options={
            # @billn revisit
            "KernelSupportPCID": False,
            "KernelVTX": True,
            "KernelX86_64VTX64BitGuests": True,
        } | DEFAULT_KERNEL_OPTIONS_X86_64,
    ),
    # BoardInfo(
    #     name="x86_64_generic_no_pcid",
    #     arch=KernelArch.X86_64,
    #     gcc_cpu="generic",
    #     loader_link_address=None,
    #     kernel_options={
    #         "KernelVTX": False,
    #         # QEMU TCG and some CPUs doesn't support PCID, so we have a
    #         # special configuration here for convenience.
    #         # For the generic configs, we want that on, as it improve context switching
    #         # performance by allowing seL4 to skip flushing the entire TLB when
    #         # switching page tables.
    #         "KernelSupportPCID": False,
    #     } | DEFAULT_KERNEL_OPTIONS_X86_64,
    # ),
    # BoardInfo(
    #     name="x86_64_generic_no_skim",
    #     arch=KernelArch.X86_64,
    #     gcc_cpu="generic",
    #     loader_link_address=None,
    #     kernel_options={
    #         "KernelVTX": False,
    #         # No mitigation against Meltdown attack for non-vulnerable processors to
    #         # prevent needless performance degradation
    #         "KernelSkimWindow": False,
    #     } | DEFAULT_KERNEL_OPTIONS_X86_64,
    # ),
    # # @billn Do we need a x86_64_generic_no_pcid_no_skim ??
)

# These then get elaborated into smp-release, smp-benchmark, and smp-debug
SUPPORTED_CONFIGS = (
    ConfigInfo(
        name="release",
        debug=False,
        kernel_options={},
        kernel_options_arch={},
    ),
    ConfigInfo(
        name="debug",
        debug=True,
        kernel_options={
            "KernelDebugBuild": True,
            "KernelPrinting": True,
            "KernelVerificationBuild": False
        },
        kernel_options_arch={},
    ),
    ConfigInfo(
        name="benchmark",
        debug=False,
        kernel_options={
            "KernelDebugBuild": False,
            "KernelVerificationBuild": False,
            "KernelBenchmarks": "track_utilisation",
            "KernelSignalFastpath": True,
        },
        kernel_options_arch={
            KernelArch.AARCH64: {
                "KernelArmExportPMUUser": True,
            },
            KernelArch.X86_64: {
                "KernelExportPMCUser": True,
                "KernelX86DangerousMSR": True,
            }
        },
    ),
)


EXAMPLES = {
    "hello": Path("example/hello"),
    "ethernet": Path("example/ethernet"),
    "passive_server": Path("example/passive_server"),
    "hierarchy": Path("example/hierarchy"),
    "timer": Path("example/timer"),
}


def elaborate_all_board_configs(board: BoardInfo) -> list[ConfigInfo]:
    elaborated_configs = list(SUPPORTED_CONFIGS)

    if board.smp_cores is not None:
        for config in SUPPORTED_CONFIGS:
            config = copy.deepcopy(config)
            config.name = f"smp-{config.name}"
            config.kernel_options |= {
                "KernelMaxNumNodes": str(board.smp_cores),
            }
            elaborated_configs.append(config)

    return elaborated_configs


def tar_filter(tarinfo: TarInfo) -> TarInfo:
    """This is used to change the tarinfo when created the .tar.gz archive.

    This ensures the tar file does not leak information from the build environment.
    """
    # Force uid/gid
    tarinfo.uid = tarinfo.gid = 0
    tarinfo.uname = tarinfo.gname = "microkit"
    # This is unlikely to be set, but force it anyway
    tarinfo.pax_headers = {}
    tarinfo.mtime = MICROKIT_EPOCH
    assert tarinfo.isfile() or tarinfo.isdir()
    # Set the permissions properly
    if tarinfo.isdir():
        tarinfo.mode = tarinfo.mode & ~0o777 | 0o744
    if tarinfo.isfile():
        if "/bin/" in tarinfo.name:
            # Assume everything in bin should be executable.
            tarinfo.mode = tarinfo.mode & ~0o777 | 0o755
        else:
            tarinfo.mode = tarinfo.mode & ~0o777 | 0o644
    return tarinfo


def get_tool_target_triple() -> str:
    host_system = host_platform.system()
    if host_system == "Linux":
        host_arch = host_platform.machine()
        if host_arch == "x86_64":
            return "x86_64-unknown-linux-musl"
        elif host_arch == "aarch64":
            return "aarch64-unknown-linux-musl"
        else:
            raise Exception(f"Unexpected Linux architecture: {host_arch}")
    elif host_system == "Darwin":
        host_arch = host_platform.machine()
        if host_arch == "x86_64":
            return "x86_64-apple-darwin"
        elif host_arch == "arm64":
            return "aarch64-apple-darwin"
        else:
            raise Exception(f"Unexpected Darwin architecture: {host_arch}")
    else:
        raise Exception(f"The platform \"{host_system}\" is not supported")


def test_tool() -> None:
    r = system(
        f"cargo test -p microkit-tool"
    )
    assert r == 0


def build_tool(tool_target: Path, target_triple: str) -> None:
    r = system(
        f"cargo build --release --locked --target {target_triple} -p microkit-tool"
    )
    assert r == 0

    tool_output = f"./target/{target_triple}/release/microkit"

    shutil.copy(tool_output, tool_target)

    tool_target.chmod(0o755)


def build_sel4(
    sel4_dir: Path,
    root_dir: Path,
    build_dir: Path,
    board: BoardInfo,
    config: ConfigInfo,
    llvm: bool
):
    """Build seL4"""
    build_dir = build_dir / board.name / config.name / "sel4"
    build_dir.mkdir(exist_ok=True, parents=True)

    sel4_install_dir = build_dir / "install"
    sel4_build_dir = build_dir / "build"

    sel4_install_dir.mkdir(exist_ok=True, parents=True)
    sel4_build_dir.mkdir(exist_ok=True, parents=True)

    print(f"Building sel4: {sel4_dir=} {root_dir=} {build_dir=} {board=} {config=}")

    config_args = [
        *board.kernel_options.items(),
        *config.kernel_options.items(),
        board.arch.as_kernel_arch_config(),
    ]
    if config.kernel_options_arch is not None:
        if board.arch in config.kernel_options_arch:
            config_args += config.kernel_options_arch[board.arch].items()
    config_strs = []
    for arg, val in sorted(config_args):
        if isinstance(val, bool):
            str_val = "ON" if val else "OFF"
        elif arg == "KernelCustomDTSOverlay":
            str_val = f"{sel4_dir.absolute()}/{val}"
        else:
            str_val = str(val)
        s = f"-D{arg}={str_val}"
        config_strs.append(s)
    config_str = " ".join(config_strs)

    target_triple = f"{board.arch.target_triple()}"

    cmd = (
        f"cmake -GNinja -DCMAKE_INSTALL_PREFIX={sel4_install_dir.absolute()} "
        f" -DPYTHON3={executable} "
        f" {config_str} "
        f"-S {sel4_dir.absolute()} -B {sel4_build_dir.absolute()}")

    if llvm:
        cmd += f" -DTRIPLE={target_triple}"
    else:
        cmd += f" -DCROSS_COMPILER_PREFIX={target_triple}-"

    r = system(cmd)
    if r != 0:
        raise Exception(f"Error configuring sel4: cmd={cmd}")

    cmd = f"cmake --build {sel4_build_dir.absolute()}"
    r = system(cmd)
    if r != 0:
        raise Exception(f"Error building sel4: cmd={cmd}")

    cmd = f"cmake --install {sel4_build_dir.absolute()}"
    r = system(cmd)
    if r != 0:
        raise Exception(f"Error installing sel4: cmd={cmd}")

    elf = sel4_install_dir / "bin" / "kernel.elf"
    elf64_dest = (
        root_dir / "board" / board.name / config.name / "elf" / "sel4.elf"
    )
    elf64_dest.unlink(missing_ok=True)
    shutil.copy(elf, elf64_dest)
    # Make output read-only
    elf64_dest.chmod(0o744)

    # qemu-system-x86_64 -kernel option only accepts a 32-bit elf32-i386 kernel image. Since seL4's
    # build process produces a 64-bit ELF, we must convert the output into a 32-bit ELF container
    # with the same code and data, allowing QEMU to load it via -kernel.
    # Otherwise, you get this "qemu-system-x86_64: Cannot load x86-64 image, give a 32bit one."
    if board.arch.is_x86():
        elf32_dest = (
            root_dir / "board" / board.name / config.name / "elf" / "sel4_32.elf"
        )
        objcopy_arg = f"-O elf32-i386 {str(elf64_dest)} {str(elf32_dest)}"
        if llvm:
            cmd = f"llvm-objcopy {objcopy_arg}"
        else:
            cmd = f"{board.arch.target_triple()}-objcopy {objcopy_arg}"
        r = system(cmd)
        if r != 0:
            raise Exception(f"Error creating 32-bit sel4 image: cmd={cmd}")
        elf32_dest.chmod(0o744)

    invocations_all = sel4_build_dir / "generated" / "invocations_all.json"
    dest = (root_dir / "board" / board.name / config.name / "invocations_all.json")
    dest.unlink(missing_ok=True)
    shutil.copy(invocations_all, dest)
    dest.chmod(0o744)

    include_dir = root_dir / "board" / board.name / config.name / "include"
    for source in ("kernel_Config", "libsel4", "libsel4/sel4_Config", "libsel4/autoconf"):
        source_dir = sel4_install_dir / source / "include"
        for p in source_dir.rglob("*"):
            if not p.is_file():
                continue
            rel = p.relative_to(source_dir)
            dest = include_dir / rel
            dest.parent.mkdir(exist_ok=True, parents=True)
            dest.unlink(missing_ok=True)
            shutil.copy(p, dest)
            dest.chmod(0o744)

    if not board.arch.is_x86():
        # only non-x86 platforms have this file to describe memory regions
        platform_gen = sel4_build_dir / "gen_headers" / "plat" / "machine" / "platform_gen.json"
        dest = root_dir / "board" / board.name / config.name / "platform_gen.json"
        dest.unlink(missing_ok=True)
        shutil.copy(platform_gen, dest)
        dest.chmod(0o744)


def build_elf_component(
    component_name: str,
    root_dir: Path,
    build_dir: Path,
    board: BoardInfo,
    config: ConfigInfo,
    llvm: bool,
    defines: List[Tuple[str, str]],
) -> None:
    """Build a specific ELF component.

    Right now this is either the loader or the monitor
    """
    sel4_dir = root_dir / "board" / board.name / config.name
    build_dir = build_dir / board.name / config.name / component_name
    build_dir.mkdir(exist_ok=True, parents=True)
    target_triple = f"{board.arch.target_triple()}"
    defines_str = " ".join(f"{k}={v}" for k, v in defines)
    defines_str += f" ARCH={board.arch.to_str()} BOARD={board.name} BUILD_DIR={build_dir.absolute()} SEL4_SDK={sel4_dir.absolute()} TARGET_TRIPLE={target_triple} LLVM={llvm}"

    if board.gcc_cpu is not None:
        defines_str += f" GCC_CPU={board.gcc_cpu}"

    r = system(
        f"{defines_str} make -C {component_name} all"
    )
    if r != 0:
        raise Exception(
            f"Error building: {component_name} for board: {board.name} config: {config.name}"
        )
    elf = build_dir / f"{component_name}.elf"
    dest = (
        root_dir / "board" / board.name / config.name / "elf" / f"{component_name}.elf"
    )
    dest.unlink(missing_ok=True)
    shutil.copy(elf, dest)
    # Make output read-only
    dest.chmod(0o744)


def build_doc(root_dir: Path):
    output = root_dir / "doc" / "microkit_user_manual.pdf"

    environ["TEXINPUTS"] = "style:"
    r = system(f'cd docs && pandoc manual.md -o ../{output}')
    assert r == 0


def build_lib_component(
    component_name: str,
    root_dir: Path,
    build_dir: Path,
    board: BoardInfo,
    config: ConfigInfo,
    llvm: bool
) -> None:
    """Build a specific library component.

    Right now this is just libmicrokit.a
    """
    sel4_dir = root_dir / "board" / board.name / config.name
    build_dir = build_dir / board.name / config.name / component_name
    build_dir.mkdir(exist_ok=True, parents=True)

    target_triple = f"{board.arch.target_triple()}"
    defines_str = f" ARCH={board.arch.to_str()} BUILD_DIR={build_dir.absolute()} SEL4_SDK={sel4_dir.absolute()} TARGET_TRIPLE={target_triple} LLVM={llvm}"

    if board.gcc_cpu is not None:
        defines_str += f" GCC_CPU={board.gcc_cpu}"

    r = system(
        f"{defines_str} make -C {component_name}"
    )
    if r != 0:
        raise Exception(
            f"Error building: {component_name} for board: {board.name} config: {config.name}"
        )
    lib = build_dir / f"{component_name}.a"
    lib_dir = root_dir / "board" / board.name / config.name / "lib"
    dest = lib_dir / f"{component_name}.a"
    dest.unlink(missing_ok=True)
    shutil.copy(lib, dest)
    # Make output read-only
    dest.chmod(0o744)

    link_script = Path(component_name) / "microkit.ld"
    dest = lib_dir / "microkit.ld"
    dest.unlink(missing_ok=True)
    shutil.copy(link_script, dest)
    # Make output read-only
    dest.chmod(0o744)

    include_dir = root_dir / "board" / board.name / config.name / "include"
    source_dir = Path(component_name) / "include"
    for p in source_dir.rglob("*"):
        if not p.is_file():
            continue
        rel = p.relative_to(source_dir)
        dest = include_dir / rel
        dest.parent.mkdir(exist_ok=True, parents=True)
        dest.unlink(missing_ok=True)
        shutil.copy(p, dest)
        dest.chmod(0o744)


def build_initialiser(
    component_name: str,
    root_dir: Path,
    build_dir: Path,
    board: BoardInfo,
    config: ConfigInfo,
) -> None:
    sel4_src_dir = build_dir / board.name / config.name / "sel4" / "install"

    cargo_cross_options = "-Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem"
    cargo_target = board.arch.rust_toolchain()
    rust_target_path = Path("initialiser/support/targets").absolute()

    dest = (
        root_dir / "board" / board.name / config.name / "elf" / f"{component_name}.elf"
    )

    # To save on build times, we share a single 'build target' dir for the component,
    # this means many initialiser dependencies do not have to be rebuilt unless something
    # with the seL4 headers changes or the target architecture.
    rust_target_dir = build_dir / component_name
    component_build_dir = build_dir / board.name / config.name / component_name
    component_build_dir.mkdir(exist_ok=True, parents=True)

    capdl_init_elf = rust_target_dir / cargo_target / "release" / "initialiser.elf"
    r = system(f"""
        RUSTC_BOOTSTRAP=1 \
        RUST_TARGET_PATH={rust_target_path} SEL4_PREFIX={sel4_src_dir.absolute()} \
            cargo build {cargo_cross_options} \
                --target {cargo_target} \
                --locked \
                --target-dir {rust_target_dir} \
                --release \
                -p initialiser
    """)
    if r != 0:
        raise Exception(
            f"Error building: {component_name} for board: {board.name} config: {config.name}"
        )

    dest.unlink(missing_ok=True)
    shutil.copy(capdl_init_elf, dest)
    # Make output read-only
    dest.chmod(0o744)


def main() -> None:
    parser = ArgumentParser()
    parser.add_argument("--sel4", type=Path, required=True)
    parser.add_argument("--tool-target-triple", default=get_tool_target_triple(), help="Compile the Microkit tool for this target triple")
    parser.add_argument("--llvm", action="store_true", help="Cross-compile seL4 and Microkit's run-time targets with LLVM")
    parser.add_argument("--boards", metavar="BOARDS", help="Comma-separated list of boards to support. When absent, all boards are supported.")
    parser.add_argument("--configs", metavar="CONFIGS", help="Comma-separated list of configurations to support. When absent, all configurations are supported.")
    parser.add_argument("--skip-tool", action="store_true", help="Tool will not be built")
    parser.add_argument("--skip-run-time", action="store_true", help="Run-time targets will not be built")
    parser.add_argument("--skip-sel4", action="store_true", help="seL4 will not be built")
    parser.add_argument("--skip-initialiser", action="store_true", help="Initialiser will not be built")
    parser.add_argument("--skip-docs", action="store_true", help="Docs will not be built")
    parser.add_argument("--skip-tar", action="store_true", help="SDK and source tarballs will not be built")
    # Read from the version file as unless someone has specified
    # a version, that is the source of truth
    with open("VERSION", "r") as f:
        default_version = f.read().strip()
    parser.add_argument("--version", default=default_version, help="SDK version")
    for arch in KernelArch:
        arch_str = arch.name.lower()
        parser.add_argument(f"--gcc-toolchain-prefix-{arch_str}", default=arch.target_triple(), help=f"GCC toolchain prefix when compiling for {arch_str}, e.g {arch_str}-none-elf")

    args = parser.parse_args()

    global TRIPLE_AARCH64
    global TRIPLE_RISCV
    global TRIPLE_X86_64
    TRIPLE_AARCH64 = args.gcc_toolchain_prefix_aarch64
    TRIPLE_RISCV = args.gcc_toolchain_prefix_riscv64
    TRIPLE_X86_64 = args.gcc_toolchain_prefix_x86_64

    version = args.version

    if args.boards is not None:
        supported_board_names = frozenset(board.name for board in SUPPORTED_BOARDS)
        selected_board_names = frozenset(args.boards.split(","))
        for board_name in selected_board_names:
            if board_name not in supported_board_names:
                raise Exception(f"Trying to build a board: {board_name} that does not exist in supported list.")
        selected_boards = [board for board in SUPPORTED_BOARDS if board.name in selected_board_names]
    else:
        selected_boards = SUPPORTED_BOARDS

    build_goals: list[tuple[BoardInfo, list[ConfigInfo]]] = []
    for board in selected_boards:
        elaborated_configs = elaborate_all_board_configs(board)

        if args.configs is not None:
            elaborated_config_names = frozenset(config.name for config in elaborated_configs)
            selected_config_names = frozenset(args.configs.split(","))

            if invalid_config_names := selected_config_names.difference(elaborated_config_names):
                raise Exception(
                    "You asked for invalid config(s) '{}' but board '{}' only supports config(s) '{}'"
                    .format(", ".join(invalid_config_names), board.name, ", ".join(elaborated_config_names))
                )

            elaborated_configs = [config for config in elaborated_configs if config.name in selected_config_names]

        build_goals.append((board, elaborated_configs))

    sel4_dir = args.sel4.expanduser()
    if not sel4_dir.exists():
        raise Exception(f"sel4_dir: {sel4_dir} does not exist")

    root_dir = Path("release") / f"{NAME}-sdk-{version}"
    tar_file = Path("release") / f"{NAME}-sdk-{version}.tar.gz"
    source_tar_file = Path("release") / f"{NAME}-source-{version}.tar.gz"
    dir_structure = [
        root_dir / "bin",
        root_dir / "board",
    ]
    if not args.skip_docs:
        dir_structure.append(root_dir / "doc")
    for (board, configs) in build_goals:
        board_dir = root_dir / "board" / board.name
        dir_structure.append(board_dir)
        for config in configs:
            config_dir = board_dir / config.name
            dir_structure.append(config_dir)
            dir_structure += [
                config_dir / "include",
                config_dir / "lib",
                config_dir / "elf",
            ]

    for dr in dir_structure:
        dr.mkdir(exist_ok=True, parents=True)

    with open(root_dir / "VERSION", "w+") as f:
        f.write(version + "\n")

    shutil.copy(Path("LICENSE.md"), root_dir)
    licenses_dir = Path("LICENSES")
    licenses_dest_dir = root_dir / "LICENSES"
    for p in licenses_dir.rglob("*"):
        if not p.is_file():
            continue
        rel = p.relative_to(licenses_dir)
        dest = licenses_dest_dir / rel
        dest.parent.mkdir(exist_ok=True, parents=True)
        dest.unlink(missing_ok=True)
        shutil.copy(p, dest)
        dest.chmod(0o744)

    if not args.skip_tool:
        tool_target = root_dir / "bin" / "microkit"
        test_tool()
        build_tool(tool_target, args.tool_target_triple)

    if not args.skip_docs:
        build_doc(root_dir)

    if not args.skip_run_time:
        build_dir = Path("build")
        for (board, configs) in build_goals:
            for config in configs:
                if not args.skip_sel4:
                    build_sel4(sel4_dir, root_dir, build_dir, board, config, args.llvm)
                loader_printing = 1 if config.name == "debug" else 0
                loader_defines = []
                if not board.arch.is_x86():
                    loader_defines.append(("LINK_ADDRESS", hex(board.loader_link_address)))
                    build_elf_component("loader", root_dir, build_dir, board, config, args.llvm, loader_defines)

                build_elf_component("monitor", root_dir, build_dir, board, config, args.llvm, [])
                build_lib_component("libmicrokit", root_dir, build_dir, board, config, args.llvm)
                if not args.skip_initialiser:
                    build_initialiser("initialiser", root_dir, build_dir, board, config)

    # Setup the examples
    for example, example_path in EXAMPLES.items():
        include_dir = root_dir / "example" / example
        source_dir = example_path
        for p in source_dir.rglob("*"):
            if not p.is_file():
                continue
            rel = p.relative_to(source_dir)
            dest = include_dir / rel
            dest.parent.mkdir(exist_ok=True, parents=True)
            dest.unlink(missing_ok=True)
            shutil.copy(p, dest)
            dest.chmod(0o744)

    if not args.skip_tar:
        print(f"Generating {tar_file}")
        # At this point we create a tar.gz file
        with tar_open(tar_file, "w:gz") as tar:
            tar.add(root_dir, arcname=root_dir.name, filter=tar_filter)

        # Build the source tar
        process = popen("git ls-files")
        filenames = [Path(fn.strip()) for fn in process.readlines()]
        process.close()
        source_prefix = Path(f"{NAME}-source-{version}")
        with tar_open(source_tar_file, "w:gz") as tar:
            for filename in filenames:
                tar.add(filename, arcname=source_prefix / filename, filter=tar_filter)


if __name__ == "__main__":
    main()
