#
# Copyright 2021, Breakaway Consulting Pty. Ltd.
#
# SPDX-License-Identifier: BSD-2-Clause
#
# PyOxidizer configuration file for generation sel4cp tool
def make_exe():
    dist = default_python_distribution()

    # Configure the policy
    policy = dist.make_python_packaging_policy()
    policy.extension_module_filter = "no-copyleft"
    policy.include_test = False
    policy.resources_location = "in-memory"
    policy.resources_location_fallback = None

    # Configure the config
    python_config = dist.make_python_interpreter_config()
    python_config.run_module = "sel4coreplat"

    exe = dist.to_python_executable(name="sel4cp", packaging_policy=policy, config=python_config)
    resources = exe.read_package_root(path="tool", packages=["sel4coreplat"])
    exe.add_python_resources(resources)

    return exe


def make_embedded_resources(exe):
    return exe.to_embedded_resources()


def make_install(exe):
    # Create an object that represents our installed application file layout.
    files = FileManifest()

    # Add the generated executable to our install layout in the root directory.
    files.add_python_resource(".", exe)

    return files

# Tell PyOxidizer about the build targets defined above.
register_target("exe", make_exe)
register_target("resources", make_embedded_resources, depends=["exe"], default_build_script=True)
register_target("install", make_install, depends=["exe"], default=True)

# Resolve whatever targets the invoker of this configuration file is requesting
# be resolved.
resolve_targets()
