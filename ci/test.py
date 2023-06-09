# The purpose of this script is to test the basic functionality of seL4CP and
# make sure that we have not broken anything drastic while making changes. Most
# of the time I have (accidentally) made erroneous changes, they have affected
# the tool (which is not too platform specific) and therefore while we are only
# running these tests via simulation and not real hardware, they are still
# somewhat useful.
#
# This script is intended to be both run by the CI as well as locally.
import logging
import threading
import os
import pexpect
from os import system, getcwd, mkdir
from shutil import rmtree
from json import load as json_load

from typing import Dict

CWD_DIR = getcwd()
DEFAULT_SDK_PATH = CWD_DIR + "/release/sel4cp-sdk-1.2.6"
DEFAULT_BUILD_DIR = CWD_DIR + "/test_build"
OPENSBI_PATH = CWD_DIR + "/opensbi"
LOG_FILE = CWD_DIR + "test_log"

RUNTIME_BOARDS = [
    "qemu_arm_virt",
    "qemu_arm_virt_hyp",
    "qemu_arm_virt_cortex_a72",
    "qemu_arm_virt_cortex_a72_hyp",
    # "qemu_riscv_virt",
    # "spike",
]

TEST_BUILD = [
    {
        "name": "tqma8xqp1gb_ethernet_example",
        "path": "example/tqma8xqp1gb/ethernet",
        "board": "tqma8xqp1gb",
        "configs": ["debug", "release"],
    }
]

# These are systems that we want to build and run to check the output.
# TODO: add more tests!
TEST_RUN = [
    {
        "path": "tests/hello",
        "name": "hello-world",
        "configs": ["debug"],
        "success": "hello, world",
    }
]

# TODO: potentially handle different QEMU CPUs (e.g cortex-a72, cortex-a57)
QEMU_AARCH64_VIRT_CMD = "qemu-system-aarch64 -machine virt -cpu %s -m size=2048M -chardev stdio,logfile=%s,id=char0,mux=on -serial chardev:char0 -mon chardev=char0 -nographic -device loader,file=%s,addr=0x70000000,cpu-num=0"
# @ivanv: do we need highmem=off?
QEMU_AARCH64_VIRT_HYP_CMD = "qemu-system-aarch64 -machine virt,virtualization=on,highmem=off,secure=off -cpu %s -m size=2048M -chardev stdio,logfile=%s,id=char0,mux=on -serial chardev:char0 -mon chardev=char0 -nographic -device loader,file=%s,addr=0x70000000,cpu-num=0"
QEMU_RISCV_VIRT_CMD = "qemu-system-riscv64 -machine virt -m size=3072M -nographic -bios %s"
QEMU_SPIKE_CMD = "qemu-system-riscv64 -machine spike -m size=4095M -nographic -bios %s"
SPIKE_CMD = "spike -m4095 %s"

failed_tests = []


def get_config_options(sdk_path: str, board: str, config: str) -> Dict:
    config_path = f"{sdk_path}/board/{board}/{config}/config.json"
    with open(config_path, "r") as f:
        return json_load(f)


def run_test(test_path: str, test_name: str, sdk_path: str, build_dir: str, config: str, board: str, config_options: Dict):
    global failed_tests

    test_identifier = f"{test_name}_{board}_{config}"
    test_build_dir = f"{build_dir}/{test_identifier}"
    mkdir(test_build_dir)
    # TODO: properly output stuff to log file
    print(f"BUILD TEST: {test_identifier}")
    sel4_arch = config_options["SEL4_ARCH"]
    cmd = f"make -C {test_path} IMAGE_NAME=loader.img ARCH={sel4_arch} BUILD_DIR={test_build_dir} SEL4CP_SDK={sdk_path} SEL4CP_CONFIG={config} SEL4CP_BOARD={board}"
    # On RISC-V platforms (32-bit or 64-bit) we build an OpenSBI to run the image, the Makefile's expect a path to
    # the OpenSBI source.
    if config_options["ARCH"] == "riscv":
        cmd += f" OPENSBI={OPENSBI_PATH}"

    cmd += f" > /dev/null"
    result = system(cmd)
    if result != 0:
        logging.error("TEST failed (build failed)")
        failed_tests.append(test_identifier)
        return

    if board in RUNTIME_BOARDS:
        if sel4_arch == "riscv64":
            if config_options["PLAT_SPIKE"]:
                system(QEMU_SPIKE_CMD % f"{test_build_dir}/platform/generic/firmware/fw_payload.elf")
                system(SPIKE_CMD % f"{test_build_dir}/platform/generic/firmware/fw_payload.elf")
            elif config_options["PLAT_QEMU_RISCV_VIRT"]:
                system(QEMU_RISCV_VIRT_CMD % f"{test_build_dir}/platform/generic/firmware/fw_payload.elf")
            else:
                raise Exception("Unexpected RISC-V runtime test board")
        elif sel4_arch == "aarch64":
            if config_options["PLAT_QEMU_ARM_VIRT"]:
                system(f"touch {test_build_dir}/log")
                if config_options["ARM_HYPERVISOR_SUPPORT"]:
                    qemu_template_cmd = QEMU_AARCH64_VIRT_HYP_CMD
                else:
                    qemu_template_cmd = QEMU_AARCH64_VIRT_CMD
                if config_options["ARM_CORTEX_A72"]:
                    qemu_cpu = "cortex-a72"
                elif config_options["ARM_CORTEX_A53"]:
                    qemu_cpu = "cortex-a53"
                else:
                    raise Exception("Unexpected QEMU CPU")

                qemu_test_cmd = qemu_template_cmd % (f"{qemu_cpu}", f"{test_build_dir}/log", f"{test_build_dir}/loader.img")

                print(f"RUN TEST: {test_identifier}")
                child = pexpect.spawn(qemu_test_cmd)
                try:
                    # @ivanv: fix
                    child.expect("hello, world", timeout=2)
                except pexpect.exceptions.TIMEOUT:
                    logging.error(f"Test failed (timeout exception): {test_identifier}")
                    logging.error(f"    QEMU command used was:")
                    logging.error(f"    {qemu_test_cmd}")
                    logging.error(f"Path to log file is: {test_build_dir}/log")
                    failed_tests.append(test_identifier)
            else:
                raise Exception("Unexpected AArch64 runtime test board")
        else:
            raise Exception("Unexpected architecture for runtime tests")


if __name__ == "__main__":
    # TODO: optionally allow SDK path to be given to the script as an arg?
    # TODO: take build dir as an arg
    # TODO: allow option to build specific test/test for specific platform
    # TODO: right now we're looking at: we should instead get the board list from the SDK not the Python
    build_dir = DEFAULT_BUILD_DIR
    sdk_path = DEFAULT_SDK_PATH

    if os.path.exists(build_dir):
        print(f"ERROR: build directory \"{build_dir}\" already exists.")
        exit(1)

    if not os.path.exists(sdk_path):
        print(f"ERROR: path to SDK \"{sdk_path}\" does not exist.")
        exit(1)

    supported_boards = os.listdir(sdk_path + "/board")

    mkdir(build_dir)

    for test in TEST_BUILD:
        test_path = test["path"]
        test_configs = test["configs"]
        if "board" in test:
            for config in test_configs:
                board = test["board"]
                config_options = get_config_options(sdk_path, board, config)
                run_test(test_path, test["name"], sdk_path, build_dir, config, board, config_options)
        else:
            for board in supported_boards:
                for config in test_configs:
                    config_options = get_config_options(sdk_path, board, config)
                    run_test(test_path, test["name"], sdk_path, build_dir, config, board, config_options)

    for test in TEST_RUN:
        test_path = test["path"]
        test_configs = test["configs"]
        if "board" in test:
            for config in test_configs:
                board = test["board"]
                config_options = get_config_options(sdk_path, board, config)
                run_test(test_path, test["name"], sdk_path, build_dir, config, board, config_options)
        else:
            for board in supported_boards:
                for config in test_configs:
                    config_options = get_config_options(sdk_path, board, config)
                    run_test(test_path, test["name"], sdk_path, build_dir, config, board, config_options)

    if len(failed_tests) > 0:
        print("Failed following tests: ")
        for test in failed_tests:
            print(f"    {test}")
        exit(1)
    else:
        print("All tests passed.")
        exit(0)

