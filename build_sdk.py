"""The SDK build script.

# Why Python (and not make, or something else)?

We call out to Make, but having this top-level driver script
is useful.

There are just a lot of things that are much easier in Python
than in make.

"""
from argparse import ArgumentParser
from os import popen, system
from shutil import copy
from pathlib import Path
from dataclasses import dataclass
from sys import executable
from tarfile import open as tar_open, TarInfo
from json import load as json_load
import platform as host_platform

from typing import Dict, Union, List, Tuple

NAME = "microkit"
VERSION = "1.2.6"

ENV_BIN_DIR = Path(executable).parent

MICROKIT_EPOCH = 1616367257

KERNEL_CONFIG_TYPE = Union[bool, str]
KERNEL_OPTIONS = Dict[str, KERNEL_CONFIG_TYPE]

X86_64_TOOLCHAIN = ""
AARCH64_TOOLCHAIN = "aarch64-none-elf-"
# We can use the same toolchain for both 32-bit and 64-bit RISC-V builds.
RISCV_TOOLCHAIN = "riscv64-unknown-elf-"

# @ivanv: temporary, this can be removed by looking at the architecture in gen_config.yaml
class BoardArch:
    AARCH64 = 1
    RISCV64 = 2
    RISCV32 = 3
    X86_64 = 4

# @ivanv: if we're going to have an optimised build, should we pass in -mtune as well to all the Makefiles
# for RISC-V builds?
# @ivanv: explain risc-v specifc arguments to the toolchain in the Makefiles, do the same for ARM since it's
# useful imo
# @ivanv: find a way to consistently pass in mabi and march to the various Makefiles

@dataclass
class BoardInfo:
    name: str
    arch: BoardArch
    gcc_flags: str
    loader_link_address: int
    kernel_options: KERNEL_CONFIG_TYPE
    examples: Dict[str, Path]


@dataclass
class ConfigInfo:
    name: str
    debug: bool
    kernel_options: KERNEL_CONFIG_TYPE


SUPPORTED_BOARDS = (
    BoardInfo(
        name="tqma8xqp1gb",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a35",
        loader_link_address=0x80280000,
        kernel_options = {
            "KernelPlatform": "tqma8xqp1gb",
            "KernelIsMCS": True,
            "KernelArmExportPCNTUser": True,
        },
        examples = {
            "ethernet": Path("example/tqma8xqp1gb/ethernet")
        }
    ),
    BoardInfo(
        name="zcu102",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a53",
        loader_link_address=0x40000000,
        kernel_options = {
            "KernelPlatform": "zynqmp",
            "KernelARMPlatform": "zcu102",
            "KernelIsMCS": True,
            "KernelArmHypervisorSupport": True,
            "KernelArmExportPCNTUser": True,
            "KernelAllowSMCCalls": True,
        },
        examples = {
            "hello": Path("example/zcu102/hello")
        }
    ),
    BoardInfo(
        name="imx8mq_evk",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a53",
        loader_link_address=0x41000000,
        kernel_options = {
            "KernelPlatform": "imx8mq-evk",
            "KernelIsMCS": True,
            "KernelArmExportPCNTUser": True,
            "KernelArmHypervisorSupport": True,
        },
        examples = {}
    ),
    BoardInfo(
        name="imx8mm_evk",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a53",
        loader_link_address=0x41000000,
        kernel_options = {
            "KernelPlatform": "imx8mm-evk",
            "KernelIsMCS": True,
            "KernelArmExportPCNTUser": True,
            "KernelArmHypervisorSupport": True,
        },
        examples = {}
    ),
    BoardInfo(
        name="imx8mm_evk_2_cores",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a53",
        loader_link_address=0x41000000,
        kernel_options = {
            "KernelPlatform": "imx8mm-evk",
            "KernelIsMCS": True,
            "KernelArmExportPCNTUser": True,
            "KernelMaxNumNodes": 2,
        },
        examples = {}
    ),
    BoardInfo(
        name="imx8mm_evk_4_cores",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a53",
        loader_link_address=0x41000000,
        kernel_options = {
            "KernelPlatform": "imx8mm-evk",
            "KernelIsMCS": True,
            "KernelArmExportPCNTUser": True,
            "KernelMaxNumNodes": 4,
        },
        examples = {}
    ),
    BoardInfo(
        name="imx8mm_evk_4_cores_hyp",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a53",
        loader_link_address=0x41000000,
        kernel_options = {
            "KernelPlatform": "imx8mm-evk",
            "KernelIsMCS": True,
            "KernelArmExportPCNTUser": True,
            "KernelMaxNumNodes": 4,
            "KernelArmHypervisorSupport": True,
        },
        examples = {}
    ),
    BoardInfo(
        name="ultra96v2",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a53",
        loader_link_address=0x40000000,
        kernel_options = {
            "KernelPlatform": "zynqmp",
            "KernelARMPlatform": "ultra96v2",
            "KernelIsMCS": True,
            "KernelArmExportPCNTUser": True,
        },
        examples = {
            "hello": Path("example/ultra96v2/hello")
        }
    ),
    BoardInfo(
        name="ultra96v2_hyp",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a53",
        loader_link_address=0x40000000,
        kernel_options = {
            "KernelPlatform": "zynqmp",
            "KernelARMPlatform": "ultra96v2",
            "KernelIsMCS": True,
            "KernelArmExportPCNTUser": True,
            "KernelArmHypervisorSupport": True,
            "KernelAllowSMCCalls": True,
        },
        examples = {}
    ),
    BoardInfo(
        name="qemu_arm_virt",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a53",
        loader_link_address=0x70000000,
        kernel_options = {
            "KernelPlatform": "qemu-arm-virt",
            "KernelIsMCS": True,
            "KernelArmExportPTMRUser": True,
            "KernelArmExportPCNTUser": True,
            "KernelArmHypervisorSupport": True,
            "KernelArmVtimerUpdateVOffset": False,
            "QEMU_MEMORY": 2048,
        },
        examples = {}
    ),
    BoardInfo(
        name="qemu_arm_virt_gicv3",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a53",
        loader_link_address=0x70000000,
        kernel_options = {
            "KernelPlatform": "qemu-arm-virt",
            "KernelIsMCS": True,
            "KernelArmGicV3": True,
            "KernelArmExportPTMRUser": True,
            "KernelArmExportPCNTUser": True,
            "KernelArmHypervisorSupport": True,
            "KernelArmVtimerUpdateVOffset": False,
            "QEMU_MEMORY": 2048,
        },
        examples = {}
    ),
    BoardInfo(
        name="qemu_arm_virt_cortex_a72",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a72",
        loader_link_address=0x70000000,
        kernel_options = {
            "KernelPlatform": "qemu-arm-virt",
            "KernelIsMCS": True,
            "KernelArmExportPCNTUser": True,
            "ARM_CPU": "cortex-a72",
        },
        examples = {}
    ),
    BoardInfo(
        name="qemu_arm_virt_cortex_a72_hyp",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a72",
        loader_link_address=0x70000000,
        kernel_options = {
            "KernelPlatform": "qemu-arm-virt",
            "KernelIsMCS": True,
            "ARM_CPU": "cortex-a72",
            "KernelArmHypervisorSupport": True,
            "QEMU_MEMORY": 2048,
        },
        examples = {}
    ),
    # @ivanv: there were issues with turning on
    # secondary cores in the loader with QEMU,
    # need to re-investigate.
    # BoardInfo(
    #     name="qemu_arm_virt_2_cores",
    #     arch=BoardArch.AARCH64,
    #     gcc_flags="GCC_CPU=cortex-a53",
    #     loader_link_address=0x70000000,
    #     kernel_options = {
    #         "KernelPlatform": "qemu-arm-virt",
    #         "KernelIsMCS": True,
    #         "KernelArmExportPCNTUser": True,
    #         "KernelMaxNumNodes": 2,
    #     },
    #     examples = {}
    # ),
    BoardInfo(
        name="odroidc2",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a53",
        loader_link_address=0x20000000,
        kernel_options = {
            "KernelPlatform": "odroidc2",
            "KernelIsMCS": True,
        },
        examples = {}
    ),
    BoardInfo(
        name="odroidc2_hyp",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a53",
        loader_link_address=0x20000000,
        kernel_options = {
            "KernelPlatform": "odroidc2",
            "KernelIsMCS": True,
            "KernelArmHypervisorSupport": True,
        },
        examples = {}
    ),
    BoardInfo(
        name="odroidc4",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a55",
        loader_link_address=0x20000000,
        kernel_options = {
            "KernelPlatform": "odroidc4",
            "KernelIsMCS": True,
            "KernelArmHypervisorSupport": True,
            "KernelArmVtimerUpdateVOffset": False,
            "KernelIRQReporting": False,
        },
        examples = {}
    ),
    BoardInfo(
        name="rpi3b",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a53",
        loader_link_address=0x10000000,
        kernel_options = {
            "KernelPlatform": "bcm2837",
            "KernelARMPlatform": "rpi3",
            "KernelIsMCS": True,
            # The kernel will default to AARCH32, which is why we specify AARCH64
            "KernelSel4Arch": "aarch64",
        },
        examples = {}
    ),
    BoardInfo(
        name="rpi4b",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a72",
        loader_link_address=0x10000000,
        kernel_options = {
            "KernelPlatform": "bcm2711",
            "KernelARMPlatform": "rpi4",
            "KernelIsMCS": True,
            "KernelArmHypervisorSupport": True,
            "RPI4_MEMORY": 4096,
        },
        examples = {}
    ),
    BoardInfo(
        name="jetson_tx2",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a57",
        loader_link_address=0x81000000,
        kernel_options = {
            "KernelPlatform": "tx2",
            "KernelIsMCS": True,
        },
        examples = {}
    ),
    BoardInfo(
        name="maaxboard",
        arch=BoardArch.AARCH64,
        gcc_flags="GCC_CPU=cortex-a53",
        loader_link_address=0x50000000,
        kernel_options = {
            "KernelPlatform": "maaxboard",
            "KernelIsMCS": True,
            "KernelArmExportPCNTUser": True,
        },
        examples = {
            "hello": Path("example/maaxboard/hello")
        }
    ),
    # For RISC-V the link address for the Microkit loader is dependent on the
    # previous loader. Currently for RISC-V platforms we use OpenSBI which
    # is placed at the start of memory and since we use FW_PAYLOAD, it places
    # the loader at fixed location of 2MiB after the start of memory. If you
    # were to use a different SBI implementation or not use FW_PAYLOAD with
    # OpenSBI, you will most likely have to change the loader_link_address.
    # BoardInfo(
    #     name="spike",
    #     arch=BoardArch.RISCV64,
    #     gcc_flags = "",
    #     loader_link_address=0x80200000,
    #     kernel_options = {
    #         "KernelIsMCS": True,
    #         "KernelPlatform": "spike",
    #     },
    #     examples = {
    #         "hello": Path("example/spike/hello")
    #     }
    # ),
    # BoardInfo(
    #     name="hifive_unleashed",
    #     arch=BoardArch.RISCV64,
    #     gcc_flags = "",
    #     loader_link_address=0x80200000,
    #     kernel_options = {
    #         "KernelIsMCS": True,
    #         "KernelPlatform": "hifive",
    #     },
    #     examples = {}
    # ),
    BoardInfo(
        name="qemu_riscv_virt",
        arch=BoardArch.RISCV64,
        gcc_flags = "",
        loader_link_address=0x80200000,
        kernel_options = {
            "KernelIsMCS": True,
            "KernelPlatform": "qemu-riscv-virt",
            "KernelRiscVHypervisorSupport": True,
            "KernelRiscvUseClintMtime": False, # @ivanv: fix kernel to get this working, right now it's getting overwritten
        },
        examples = {
            "hello": Path("example/qemu_riscv_virt/hello")
        }
    ),
    # BoardInfo(
    #     name="qemu_riscv_virt_hyp",
    #     arch=BoardArch.RISCV64,
    #     gcc_flags = "",
    #     loader_link_address=0x80200000,
    #     kernel_options = {
    #         "KernelIsMCS": True,
    #         "KernelPlatform": "qemu-riscv-virt",
    #     },
    #     examples = {}
    # ),
    # BoardInfo(
    #     name="qemu_riscv_virt_no_hyp_32",
    #     arch=BoardArch.RISCV32,
    #     gcc_flags = "",
    #     loader_link_address=0x80200000,
    #     kernel_options = {
    #         "KernelIsMCS": True,
    #         "KernelPlatform": "qemu-riscv-virt",
    #         "KernelSel4Arch": "riscv32",
    #     },
    #     examples = {}
    # ),
    BoardInfo(
        name="star64",
        arch=BoardArch.RISCV64,
        gcc_flags = "",
        loader_link_address=0x60000000,
        kernel_options = {
            "KernelIsMCS": True,
            "KernelPlatform": "star64",
        },
        examples = {
            "hello": Path("example/star64/hello")
        }
    ),
    BoardInfo(
        name="polarfire",
        arch=BoardArch.RISCV64,
        gcc_flags = "",
        loader_link_address=0x80200000,
        kernel_options = {
            "KernelIsMCS": True,
            "KernelPlatform": "polarfire",
        },
        examples = {
            "hello": Path("example/polarfire/hello")
        }
    ),
    # BoardInfo(
    #     name="x86_64",
    #     arch=BoardArch.X86_64,
    #     gcc_flags = "",
    #     loader_link_address=0x80200000,
    #     kernel_options = {
    #         "KernelIsMCS": True,
    #         "KernelPlatform": "pc99",
    #         "KernelSel4Arch": "x86_64",
    #     },
    #     examples = {}
    # ),
)

SUPPORTED_CONFIGS = (
    ConfigInfo(
        name="release",
        debug=False,
        kernel_options = {},
    ),
    ConfigInfo(
        name="debug",
        debug=True,
        kernel_options = {
            "KernelDebugBuild": True,
            "KernelPrinting": True,
            "KernelVerificationBuild": False
        }
    ),
    # @ivanv: This has ARM specific kernel options
    ConfigInfo(
        name="benchmark",
        debug=False,
        kernel_options = {
            "KernelDebugBuild": False,
            "KernelVerificationBuild": False,
            "KernelBenchmarks": "track_utilisation",
            "KernelArmExportPMUUser": True,
            # Enable signal fastpath for sDDF benchmarking
            "KernelSignalFastpath": True,
        },
    ),
)


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
        tarinfo.mode = tarinfo.mode & ~0o777 | 0o775
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
        return "x86_64-unknown-linux-musl"
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
        f"{executable} -m unittest discover -s tool -v"
    )
    assert r == 0

def build_tool(tool_target: Path, target_triple: str) -> None:
    pyoxidizer = ENV_BIN_DIR / "pyoxidizer"
    if not pyoxidizer.exists():
        raise Exception("pyoxidizer does not appear to be installed in your Python environment")

    r = system(
        f"{pyoxidizer} build --release --path tool --target-triple {target_triple}"
    )
    assert r == 0

    tool_output = f"./tool/build/{target_triple}/release/install/microkit"

    r = system(f"strip {tool_output}")
    assert r == 0

    copy(tool_output, tool_target)


def build_sel4(
    sel4_dir: Path,
    root_dir: Path,
    build_dir: Path,
    board: BoardInfo,
    config: ConfigInfo,
) -> None:
    """Build seL4"""
    build_dir = build_dir / board.name / config.name / "sel4"
    build_dir.mkdir(exist_ok=True, parents=True)

    sel4_install_dir = build_dir / "install"
    sel4_build_dir = build_dir / "build"

    sel4_install_dir.mkdir(exist_ok=True, parents=True)
    sel4_build_dir.mkdir(exist_ok=True, parents=True)

    print(f"Building seL4: {sel4_dir=} {root_dir=} {build_dir=} {board=} {config=}")

    config_args = list(board.kernel_options.items()) + list(config.kernel_options.items())
    config_strs = []
    for arg, val in sorted(config_args):
        if isinstance(val, bool):
            str_val = "ON" if val else "OFF"
        else:
            str_val = str(val)
        s = f"-D{arg}={str_val}"
        config_strs.append(s)
    config_str = " ".join(config_strs)

    if board.arch == BoardArch.RISCV64 or board.arch == BoardArch.RISCV32:
        toolchain_config = f"-DCROSS_COMPILER_PREFIX={RISCV_TOOLCHAIN}"
    elif board.arch == BoardArch.AARCH64:
        toolchain_config = f"-DCROSS_COMPILER_PREFIX={AARCH64_TOOLCHAIN}"
    elif board.arch == BoardArch.X86_64:
        if host_arch != "x86_64":
            assert False, "@ivanv: Figure out cross-compiling to x86-64"
        else:
            toolchain_config = ""
    else:
        raise Exception(f"Unexpected board arch: {board.arch}")

    cmd = (
        f"cmake -GNinja -DCMAKE_INSTALL_PREFIX={sel4_install_dir.absolute()} "\
        f" -DPYTHON3={executable} " \
        f" {config_str} " \
        f" {toolchain_config} " \
        f"-S {sel4_dir.absolute()} -B {sel4_build_dir.absolute()}")

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
    dest = (
        root_dir / "board" / board.name / config.name / "elf" / "sel4.elf"
    )
    dest.unlink(missing_ok=True)
    copy(elf, dest)

    # @ivanv: comment, this is hard to follow
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
            copy(p, dest)


def build_elf_component(
    component_name: str,
    root_dir: Path,
    build_dir: Path,
    board: BoardInfo,
    config: ConfigInfo,
    defines: List[Tuple[str, str]]
) -> None:
    """Build a specific ELF component.

    Right now this is either the loader or the monitor
    """
    sel4_dir = root_dir / "board" / board.name / config.name
    build_dir = build_dir / board.name / config.name / component_name
    build_dir.mkdir(exist_ok=True, parents=True)
    defines_str = " ".join(f"{k}={v}" for k, v in defines)

    if board.arch == BoardArch.AARCH64:
        arch_args = f"ARCH=aarch64 TOOLCHAIN={AARCH64_TOOLCHAIN}"
    elif board.arch == BoardArch.RISCV64:
        arch_args = f"ARCH=riscv64 TOOLCHAIN={RISCV_TOOLCHAIN}"
    elif board.arch == BoardArch.RISCV32:
        arch_args = f"ARCH=riscv32 TOOLCHAIN={RISCV_TOOLCHAIN}"
    elif board.arch == BoardArch.X86_64:
        arch_args = f"ARCH=x86_64 TOOLCHAIN={X86_64_TOOLCHAIN}"
    else:
        raise Exception(f"Unexpected arch given: {board.arch}", board.arch)

    build_cmd = f"BOARD={board.name} BUILD_DIR={build_dir.absolute()} {arch_args} {board.gcc_flags} SEL4_SDK={sel4_dir.absolute()} {defines_str} make -C {component_name}"
    r = system(build_cmd)
    if r != 0:
        raise Exception(
            f"Error building: {component_name} for board: {board.name} config: {config.name}"
        )
    elf = build_dir / f"{component_name}.elf"
    dest = (
        root_dir / "board" / board.name / config.name / "elf" / f"{component_name}.elf"
    )
    dest.unlink(missing_ok=True)
    copy(elf, dest)


def build_doc(root_dir):
    output = root_dir / "doc" / "microkit_user_manual.pdf"

    r = system(f'pandoc docs/manual.md -o {output}')
    assert r == 0


def build_lib_component(
    component_name: str,
    root_dir: Path,
    build_dir: Path,
    board: BoardInfo,
    config: ConfigInfo,
) -> None:
    """Build a specific library component.

    Right now this is just libsel4.a
    """
    sel4_dir = root_dir / "board" / board.name / config.name
    build_dir = build_dir / board.name / config.name / component_name
    build_dir.mkdir(exist_ok=True, parents=True)

    if board.arch == BoardArch.AARCH64:
        arch_args = f"ARCH=aarch64 TOOLCHAIN={AARCH64_TOOLCHAIN}"
    elif board.arch == BoardArch.RISCV64:
        arch_args = f"ARCH=riscv64 TOOLCHAIN={RISCV_TOOLCHAIN}"
    elif board.arch == BoardArch.RISCV32:
        arch_args = f"ARCH=riscv32 TOOLCHAIN={RISCV_TOOLCHAIN}"
    else:
        raise Exception(f"Unexpected arch given: {board.arch}", board.arch)

    build_cmd = f"BUILD_DIR={build_dir.absolute()} {arch_args} {board.gcc_flags} SEL4_SDK={sel4_dir.absolute()} make -C {component_name}"
    r = system(build_cmd)
    if r != 0:
        raise Exception(
            f"Error building: {component_name} for board: {board.name} config: {config.name}"
        )
    lib = build_dir / f"{component_name}.a"
    lib_dir = root_dir / "board" / board.name / config.name / "lib"
    dest = lib_dir / f"{component_name}.a"
    dest.unlink(missing_ok=True)
    copy(lib, dest)


    link_script = Path(component_name) / "microkit.ld"
    dest = lib_dir / "microkit.ld"
    dest.unlink(missing_ok=True)
    copy(link_script, dest)

    crt0 = build_dir / "crt0.o"
    dest = lib_dir / "crt0.o"
    dest.unlink(missing_ok=True)
    copy(crt0, dest)

    include_dir = root_dir / "board" / board.name / config.name / "include"
    source_dir = Path(component_name) / "include"
    for p in source_dir.rglob("*"):
        if not p.is_file():
            continue
        rel = p.relative_to(source_dir)
        dest = include_dir / rel
        dest.parent.mkdir(exist_ok=True, parents=True)
        dest.unlink(missing_ok=True)
        copy(p, dest)


def build_sel4_config_component(
    root_dir: Path,
    build_dir: Path,
    board: BoardInfo,
    config: ConfigInfo,
) -> Dict:
    # Here we are just copying the auto-generated kernel config, "gen_config.json".
    # This is because it is needed for the tool so it can deal with seL4/platform
    # specific configuration. It is also used in this build script.
    sel4_build_dir = build_dir / board.name / config.name / "sel4" / "build"
    sel4_gen_config = sel4_build_dir / "gen_config" / "kernel" / "gen_config.json"
    dest = root_dir / "board" / board.name / config.name / "config.json"
    with open(sel4_gen_config, "r") as f:
        # Load the generated kernel configuration into a dictionary
        sel4_config = json_load(f)

    dest.unlink(missing_ok=True)
    copy(sel4_gen_config, dest)

    return sel4_config


def main() -> None:
    parser = ArgumentParser()
    parser.add_argument("--sel4", type=Path, required=True)
    parser.add_argument("--tool-rebuild", action="store_true", default=False, help="Force a rebuild of the Microkit tool")
    parser.add_argument("--tool-target-triple", default=get_tool_target_triple(), help="Target triple of the Microkit tool")
    parser.add_argument("--filter-boards", help="List of boards to build SDK for (comma separated)")
    parser.add_argument("--no-archive", action="store_true",
        help="Disable archiving, useful when you are impatient like myself and don't want to wait for the SDK to be archived again")
    args = parser.parse_args()
    sel4_dir = args.sel4.expanduser()
    if not sel4_dir.exists():
        raise Exception(f"sel4_dir: {sel4_dir} does not exist")


    root_dir = Path("release") / f"{NAME}-sdk-{VERSION}"
    tar_file = Path("release") / f"{NAME}-sdk-{VERSION}.tar.gz"
    source_tar_file = Path("release") / f"{NAME}-source-{VERSION}.tar.gz"
    dir_structure = [
        root_dir / "doc",
        root_dir / "bin",
        root_dir / "board",
    ]

    if args.filter_boards:
        filter_board_names = args.filter_boards.split(",")
        supported_board_names = [board.name for board in SUPPORTED_BOARDS]
        # Check that we are filtering boards that actually are supported
        for board in filter_board_names:
            if board not in supported_board_names:
                raise Exception(f"Trying to build a board: {board} that does not exist in supported list.")
        # Filter the boards
        selected_boards = list(filter(lambda b : b.name in filter_board_names, SUPPORTED_BOARDS))
    else:
        selected_boards = SUPPORTED_BOARDS

    for board in selected_boards:
        board_dir = root_dir / "board" / board.name
        dir_structure.append(board_dir)
        for config in SUPPORTED_CONFIGS:
            config_dir = board_dir / config.name
            dir_structure.append(config_dir)
            dir_structure += [
                config_dir / "include",
                config_dir / "lib",
                config_dir / "elf",
            ]

    for dr in dir_structure:
        dr.mkdir(exist_ok=True, parents=True)

    copy(Path("LICENSE"), root_dir)

    tool_target = root_dir / "bin" / "microkit"

    if not tool_target.exists() or args.tool_rebuild:
        test_tool()
        build_tool(tool_target, args.tool_target_triple)

    build_doc(root_dir)

    build_dir = Path("build")
    for board in selected_boards:
        for config in SUPPORTED_CONFIGS:
            build_sel4(sel4_dir, root_dir, build_dir, board, config)
            sel4_config = build_sel4_config_component(root_dir, build_dir, board, config)
            # Get the defines needed by the loader from the auto-generated seL4 config.
            assert "MAX_NUM_NODES" in sel4_config
            num_cpus = sel4_config["MAX_NUM_NODES"]
            loader_defines = [
                ("LINK_ADDRESS", hex(board.loader_link_address)),
                ("NUM_CPUS", num_cpus),
            ]
            if board.arch == BoardArch.RISCV64:
                # On RISC-V the loader needs to know the expected first HART ID.
                assert "FIRST_HART_ID" in sel4_config
                loader_defines.append(("FIRST_HART_ID", sel4_config["FIRST_HART_ID"]))
            elif board.arch == BoardArch.AARCH64:
                if sel4_config["ARM_PA_SIZE_BITS_40"]:
                    pa_size_bits = 40
                elif sel4_config["ARM_PA_SIZE_BITS_44"]:
                    pa_size_bits = 44
                else:
                    raise Exception("Expected seL4 generated config to define number of physical address bits")
                loader_defines.append(("PA_SIZE_BITS", pa_size_bits))
            build_elf_component("loader", root_dir, build_dir, board, config, loader_defines)
            build_elf_component("monitor", root_dir, build_dir, board, config, [])
            build_lib_component("libmicrokit", root_dir, build_dir, board, config)
        # Setup the examples
        for example, example_path in board.examples.items():
            include_dir = root_dir / "board" / board.name / "example" / example
            source_dir = example_path
            for p in source_dir.rglob("*"):
                if not p.is_file():
                    continue
                rel = p.relative_to(source_dir)
                dest = include_dir / rel
                dest.parent.mkdir(exist_ok=True, parents=True)
                dest.unlink(missing_ok=True)
                copy(p, dest)

    if not args.no_archive:
        # At this point we create a tar.gz file
        with tar_open(tar_file, "w:gz") as tar:
            tar.add(root_dir, arcname=root_dir.name, filter=tar_filter)

        # Build the source tar
        process = popen("git ls-files")
        filenames = [Path(fn.strip()) for fn in process.readlines()]
        process.close()
        source_prefix = Path(f"{NAME}-source-{VERSION}")
        with tar_open(source_tar_file, "w:gz") as tar:
            for filename in filenames:
                tar.add(filename, arcname=source_prefix / filename, filter=tar_filter)

if __name__ == "__main__":
    main()
