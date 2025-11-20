# Copyright 2021, Breakaway Consulting Pty. Ltd.
# SPDX-License-Identifier: BSD-2-Clause

"""Build a specific example during development.

This is designed to make it easy to build and run examples during development.
"""
from argparse import ArgumentParser
from os import environ, system
from pathlib import Path
from shutil import rmtree
from subprocess import run

CWD = Path(__file__).parent
BUILD_DIR = CWD / "tmp_build"


def find_releases():
    releases = []
    for f in (CWD / "release").iterdir():
        if not f.is_dir():
            continue
        if not f.name.startswith("microkit-sdk-"):
            # All directories in here should match this, but
            # skip just iun case someone added junk
            continue
        releases.append(f)

    def release_sort_key(rel):
        ver_str = rel.name.split("-")[2]
        ver = tuple(int(x) for x in ver_str.split("."))
        return ver

    releases.sort(key=release_sort_key, reverse=True)
    return releases


def main():
    parser = ArgumentParser()
    parser.add_argument(
        "--rebuild",
        action="store_true",
        default=False,
        help="Force a rebuild of the example",
    )
    parser.add_argument(
        "--example-from-sdk",
        action="store_true",
        default=False,
        help="Build the example from the SDK build rather than directly from source directory",
    )
    parser.add_argument(
        "--board",
        help="Target board",
        required=True
    )
    parser.add_argument(
        "--example",
        help="Example to build",
        required=True
    )
    parser.add_argument(
        "--config",
        default="debug",
        help="Config option to be passed to the tool"
    )
    parser.add_argument(
        "--llvm",
        action="store_true",
        help="Build with LLVM/Clang toolchain"
    )
    args = parser.parse_args()

    # TODO: Support choosing a release by specifying on command line
    releases = find_releases()
    release = releases[0]

    if args.rebuild and BUILD_DIR.exists():
        rmtree(BUILD_DIR)

    if not BUILD_DIR.exists():
        BUILD_DIR.mkdir()

    tool_rebuild = f"cd {CWD / 'tool/microkit'} && cargo build --release"
    r = system(tool_rebuild)
    assert r == 0

    make_env = environ.copy()
    make_env["BUILD_DIR"] = str(BUILD_DIR.absolute())
    make_env["MICROKIT_BOARD"] = args.board
    make_env["MICROKIT_CONFIG"] = args.config
    make_env["MICROKIT_SDK"] = str(release)
    make_env["MICROKIT_TOOL"] = (CWD / "target/release/microkit").absolute()
    make_env["LLVM"] = str(args.llvm)

    # Choose the makefile based on the `--example-from-sdk` command line flag
    makefile_directory = (
        f"{release}/example/{args.example}"
        if args.example_from_sdk
        else f"{CWD.absolute()}/example/{args.example}"
    )

    cmd = ["make", "-C", makefile_directory]

    run(cmd, env=make_env, check=True)


if __name__ == "__main__":
    main()
