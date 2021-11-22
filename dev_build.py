"""Build a specific example during development.

This is designed to make it easy to build and run examples during development.
"""
from argparse import ArgumentParser
from os import environ
from pathlib import Path
from shutil import rmtree
from subprocess import run
from sys import executable

CWD = Path.cwd()
BUILD_DIR = CWD / "tmp_build"

sel4cp_config = "debug"


def find_releases():
    releases = []
    for f in (CWD / "release").iterdir():
        if not f.is_dir():
            continue
        if not f.name.startswith("sel4cp-sdk-"):
            # All directories in here should match this, but
            # skip just iun case someone added junk
            continue
        releases.append(f)

    def release_sort_key(rel):
        ver_str = rel.name.split("-")[-1]
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
        "--tool-from-sdk",
        action="store_true",
        default=False,
        help="Build the using the tool from the SDK rather than directly from the Python source",
    )
    parser.add_argument(
        "--board",
        help="Target board"
    )
    parser.add_argument(
        "--example",
        help="Example to build"
    )
    args = parser.parse_args()

    # TODO: Support choosing a release by specifying on command line
    releases = find_releases()
    release = releases[0]

    if args.rebuild and BUILD_DIR.exists():
        rmtree(BUILD_DIR)

    if not BUILD_DIR.exists():
        BUILD_DIR.mkdir()

    make_env = environ.copy()
    make_env["BUILD_DIR"] = str(BUILD_DIR.absolute())
    make_env["SEL4CP_BOARD"] = args.board
    make_env["SEL4CP_CONFIG"] = sel4cp_config
    make_env["SEL4CP_SDK"] = str(release)

    # Choose the makefile based on the `--example-from-sdk` command line flag
    makefile_directory = (
        f"{release}/board/{args.board}/example/{args.example}"
        if args.example_from_sdk
        else f"{CWD.absolute()}/example/{args.board}/{args.example}"
    )

    if not args.tool_from_sdk:
        make_env["PYTHONPATH"] = str(CWD / "tool")
        make_env["SEL4CP_TOOL"] = f"{executable} -m sel4coreplat"

    cmd = ["make", "-C", makefile_directory]

    run(cmd, env=make_env)


if __name__ == "__main__":
    main()
