#!/usr/bin/env bash

export PROJ_ROOT=../../..
export BUILD_DIR=$PROJ_ROOT/tmp_build/hello
mkdir -p $BUILD_DIR
export SEL4CP_SDK=$PROJ_ROOT/release/sel4cp-sdk-1.2.6
export SEL4CP_BOARD=zcu102
export SEL4CP_CONFIG=debug
export PYTHONPATH=$PROJ_ROOT/tool
export SEL4CP_TOOL="python -m sel4coreplat"

make