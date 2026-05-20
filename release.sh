#!/bin/bash

set -e
set -x

if [ "$#" -ne 1 ]; then
    echo "./release.sh microkit_version"
    exit 1
fi

SDK_VERSION=$1
RELEASES="${PWD}/releases/"

MACOS_SIGN="${PWD}/../macos_sign.sh"

unpack_github_sdk() {
    SDK_TARGET=$1
    unzip microkit-sdk-${SDK_VERSION}-${SDK_TARGET}.zip
    tar xf microkit-sdk-${SDK_VERSION}-${SDK_TARGET}.tar.gz
    mv microkit-sdk-${SDK_VERSION} ${RELEASES}/microkit-sdk-${SDK_VERSION}-${SDK_TARGET}
}

package_sdk() {
    SDK_TARGET=$1
    SDK_NAME="microkit-sdk-${SDK_VERSION}-${SDK_TARGET}"
    pushd ${RELEASES}
    rm -rf microkit-sdk-${SDK_VERSION}
    cp -r ${SDK_NAME} microkit-sdk-${SDK_VERSION}
    # If we use BSD tar to package the SDK but then untar using GNU tar (e.g on Linux) then
    # we'll see stupid warnings and it will look unprofessional.
    # So use Nix so we definitely get GNU tar.
    nix-shell -p gnutar gzip --command "tar cf ${SDK_NAME}.tar microkit-sdk-${SDK_VERSION} && gzip ${SDK_NAME}.tar"
    rm -rf microkit-sdk-${SDK_VERSION}
    popd
}

# This will change the tool binary itself, so it is important to call this *before* the
# taring of the SDKs happens.
macos_sign() {
    SDK_TARGET=$1

    SDK_DIR="${RELEASES}/microkit-sdk-${SDK_VERSION}-${SDK_TARGET}"

    BINARY="${SDK_DIR}/bin/microkit"

    ${MACOS_SIGN} ${BINARY}
}

gpg_sign() {
    SDK_TARGET=$1
    gpg -ab ${RELEASES}/microkit-sdk-${SDK_VERSION}-${SDK_TARGET}.tar.gz
}

mkdir -p ${RELEASES}

unpack_github_sdk "linux-x86-64"
unpack_github_sdk "linux-aarch64"
unpack_github_sdk "macos-aarch64"
unpack_github_sdk "macos-x86-64"

macos_sign "macos-aarch64"
macos_sign "macos-x86-64"

package_sdk "linux-x86-64"
package_sdk "linux-aarch64"
package_sdk "macos-aarch64"
package_sdk "macos-x86-64"

gpg_sign "linux-x86-64"
gpg_sign "linux-aarch64"
gpg_sign "macos-aarch64"
gpg_sign "macos-x86-64"
