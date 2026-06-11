#!/bin/bash

# Copyright 2024, UNSW
# SPDX-License-Identifier: BSD-2-Clause

set -e

VERSION=`cat VERSION`

HEAD=`git rev-parse --short HEAD`

if ! LATEST_TAG=`git describe --tags --abbrev=0`; then
    VERSION="$VERSION.unknown+$HEAD"
elif ! NUM_COMMITS=`git rev-list --count $LATEST_TAG..HEAD`; then
    VERSION="$VERSION.unknown+$HEAD"
elif [ $NUM_COMMITS -eq 0 ]; then
    echo "$VERSION"
else
    VERSION="$VERSION.$NUM_COMMITS+$HEAD"
fi

echo "SDK Version is '${VERSION}'"

if [ -n "${GITHUB_ENV}" ]; then
    echo "SDK_VERSION=${VERSION}" >> "${GITHUB_ENV}"
fi
if [ -n "${GITHUB_OUTPUT}" ]; then
    echo "SDK_VERSION=${VERSION}" >> "${GITHUB_OUTPUT}"
fi
