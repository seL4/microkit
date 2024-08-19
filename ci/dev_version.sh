#!/bin/bash

# Copyright 2024, UNSW
# SPDX-License-Identifier: BSD-2-Clause

set -e

VERSION=`cat VERSION`
LATEST_TAG=`git describe --tags --abbrev=0`
NUM_COMMITS=`git rev-list --count $LATEST_TAG..HEAD`
HEAD=`git rev-parse --short HEAD`

echo "$VERSION.$NUM_COMMITS+$HEAD"
