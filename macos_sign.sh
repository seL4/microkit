#!/bin/sh

set -e

BINARY_PATH=$1

if [ "$#" -ne 1 ]; then
    echo "./macos_sign.sh /path/to/binary"
    exit 1
fi

echo "INFO: signing '$BINARY_PATH'"

if [[ -z $DEVELOPER_ID ]]; then
    echo "ERROR: need to set DEVELOPER_ID"
    exit 1
fi

if [[ -z $APP_PASSWORD_KEYCHAIN_ID ]]; then
    echo "ERROR: need to set APP_PASSWORD_KEYCHAIN_ID"
    exit 1
fi

codesign -s "${DEVELOPER_ID}" -f --timestamp -o runtime -i "systems.sel4.microkit" "${BINARY_PATH}"

rm -rf microkit.zip
zip microkit.zip ${BINARY_PATH}

xcrun notarytool submit microkit.zip --keychain-profile "${APP_PASSWORD_KEYCHAIN_ID}" --verbose
