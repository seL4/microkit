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
from yaml import load  as yaml_load, Loader as YamlLoader

from typing import Dict, Union, List, Tuple

NAME = "sel4cp"
VERSION = "1.2.6"

ENV_BIN_DIR = Path(executable).parent

SEL4CP_EPOCH = 1616367257

KERNEL_CONFIG_TYPE = Union[bool, str]
KERNEL_OPTIONS = Dict[str, KERNEL_CONFIG_TYPE]

AARCH64_TOOLCHAIN = "aarch64-none-elf-"
RISCV64_TOOLCHAIN = "riscv64-unknown-elf-"

# @ivanv: temporary, this can be removed by looking at the architecture in gen_config.yaml
class BoardArch:
    AARCH64 = 1
    RISCV64 = 2

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
            "KernelArmExportPCNTUser": True,
        },
        examples = {
            "hello": Path("example/zcu102/hello")
        }
    ),
    # For RISC-V the link address for the seL4CP loader is dependent on the
    # previous loader. Currently for RISC-V platforms we use OpenSBI which
    # is placed at the start of memory and since we use FW_PAYLOAD, it places
    # the loader at fixed location of 2MiB after the start of memory. If you
    # were to use a different SBI implementation or not use FW_PAYLOAD with
    # OpenSBI, you will most likely have to change the loader_link_address.
    BoardInfo(
        name="spike",
        arch=BoardArch.RISCV64,
        gcc_flags = "",
        loader_link_address=0x80200000,
        kernel_options = {
            "KernelIsMCS": True,
            "KernelPlatform": "spike",
        },
        examples = {
            "hello": Path("example/spike/hello")
        }
    ),
    BoardInfo(
        name="hifive_unleashed",
        arch=BoardArch.RISCV64,
        gcc_flags = "",
        loader_link_address=0x80200000,
        kernel_options = {
            "KernelIsMCS": True,
            "KernelPlatform": "hifive",
        },
        examples = {
            "hello": Path("example/hifive/hello")
        }
    ),
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
)


def tar_filter(tarinfo: TarInfo) -> TarInfo:
    """This is used to change the tarinfo when created the .tar.gz archive.

    This ensures the tar file does not leak information from the build environment.
    """
    # Force uid/gid
    tarinfo.uid = tarinfo.gid = 0
    tarinfo.uname = tarinfo.gname = "sel4cp"
    # This is unlikely to be set, but force it anyway
    tarinfo.pax_headers = {}
    tarinfo.mtime = SEL4CP_EPOCH
    assert tarinfo.isfile() or tarinfo.isdir()
    # Set the permissions properly
    if tarinfo.isdir():
        tarinfo.mode = tarinfo.mode & ~0o777 | 0o557
    if tarinfo.isfile():
        if "/bin/" in tarinfo.name:
            # Assume everything in bin should be executable.
            tarinfo.mode = tarinfo.mode & ~0o777 | 0o755
        else:
            tarinfo.mode = tarinfo.mode & ~0o777 | 0o644
    return tarinfo


def test_tool() -> None:
    r = system(
        f"{executable} -m unittest discover -s tool -v"
    )
    assert r == 0

def build_tool(tool_target: Path) -> None:
    pyoxidizer = ENV_BIN_DIR / "pyoxidizer"
    if not pyoxidizer.exists():
        raise Exception("pyoxidizer does not appear to be installed in your Python environment")
    r = system(
        f"{pyoxidizer} build --release --path tool --target-triple x86_64-unknown-linux-musl"
    )
    assert r == 0

    tool_output = "./tool/build/x86_64-unknown-linux-musl/release/install/sel4cp"

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

    print(f"Building sel4: {sel4_dir=} {root_dir=} {build_dir=} {board=} {config=}")

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

    platform = board.name
    cmd = (
        f"cmake -GNinja -DCMAKE_INSTALL_PREFIX={sel4_install_dir.absolute()} "\
        f" -DPYTHON3={executable} " \
        f" -DKernelPlatform={platform} {config_str} " \
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
    # Make output read-only
    dest.chmod(0o444)

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
            dest.chmod(0o444)


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
        arch_args = f"ARCH=riscv64 TOOLCHAIN={RISCV64_TOOLCHAIN}"
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
    # Make output read-only
    dest.chmod(0o444)


def build_doc(root_dir):
    output = root_dir / "doc" / "sel4cp_user_manual.pdf"

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
        arch_args = f"ARCH=riscv64 TOOLCHAIN={RISCV64_TOOLCHAIN}"
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
    # Make output read-only
    dest.chmod(0o444)


    link_script = Path(component_name) / "sel4cp.ld"
    dest = lib_dir / "sel4cp.ld"
    dest.unlink(missing_ok=True)
    copy(link_script, dest)
    # Make output read-only
    dest.chmod(0o444)

    crt0 = build_dir / "crt0.o"
    dest = lib_dir / "crt0.o"
    dest.unlink(missing_ok=True)
    copy(crt0, dest)
    # Make output read-only
    dest.chmod(0o444)

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
        dest.chmod(0o444)


def build_kernel_config_component(
    root_dir: Path,
    build_dir: Path,
    board: BoardInfo,
    config: ConfigInfo,
) -> None:
    # Here we are just copying the auto-generated kernel config, "gen_config.yaml".
    sel4_build_dir = build_dir / board.name / config.name / "sel4" / "build"
    sel4_gen_config = sel4_build_dir / "gen_config" / "kernel" / "gen_config.yaml"
    dest = root_dir / "board" / board.name / config.name / "config.yaml"
    with open(sel4_gen_config, "r") as f:
        sel4_config = yaml_load(f, Loader=YamlLoader)

    dest.unlink(missing_ok=True)
    copy(sel4_gen_config, dest)
    dest.chmod(0o444)


def main() -> None:
    parser = ArgumentParser()
    parser.add_argument("--sel4", type=Path, required=True)
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
    for board in SUPPORTED_BOARDS:
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

    tool_target = root_dir / "bin" / "sel4cp"

    if not tool_target.exists():
        test_tool()
        build_tool(tool_target)

    build_doc(root_dir)

    build_dir = Path("build")
    for board in SUPPORTED_BOARDS:
        for config in SUPPORTED_CONFIGS:
            build_sel4(sel4_dir, root_dir, build_dir, board, config)
            loader_defines = [
                ("LINK_ADDRESS", hex(board.loader_link_address))
            ]
            kernel_config = build_kernel_config_component(root_dir, build_dir, board, config)
            build_elf_component("loader", root_dir, build_dir, board, config, loader_defines)
            build_elf_component("monitor", root_dir, build_dir, board, config, [])
            build_lib_component("libsel4cp", root_dir, build_dir, board, config)
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
                dest.chmod(0o444)

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
