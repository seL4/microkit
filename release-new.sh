#!/usr/bin/env bash

set -euo pipefail
set -x

SDK_VERSION=$(cat VERSION)

# Tarfiles like microkit-sdk-2.3.1-linux-aarch64.zip downloaded off GitHub Actions CI

if ! tar --version | grep "GNU tar"; then
  echo >&2 "GNU tar expected - weird archives produced otherwise"
fi

names=(linux-aarch64 linux-x86-64 macos-aarch64 macos-x86-64)

TEMP_FOLDER=release-temp-2
rm -rf "$TEMP_FOLDER"
mkdir "$TEMP_FOLDER"
pushd "$TEMP_FOLDER"

for name in ${names[@]}; do
  unzip "../microkit-sdk-${SDK_VERSION}-${name}.zip"
done

MAC_MINI_SSH="julia@foundation-mac-mini-m2-16gb-1.keg"

for name in ${names[@]}; do
  if [[ "$name" =~ ^macos.* ]]; then
    mkdir "$name/"
    tar xzf "microkit-sdk-${SDK_VERSION}-${name}.tar.gz" -C "$name/"
    rm "microkit-sdk-${SDK_VERSION}-${name}.tar.gz"
    scp "$name/microkit-sdk-${SDK_VERSION}/bin/microkit" "$MAC_MINI_SSH":"microkit-bin-$name-${SDK_VERSION}"
  fi
done

cmd="true"
for name in ${names[@]}; do
  if [[ "$name" =~ ^macos.* ]]; then
    cmd+=" && bash macos_sign.sh microkit-bin-$name-${SDK_VERSION}"
  fi
done

# This doesn't work due to Apple Keychain Nonsense
# ssh "$MAC_MINI_SSH" "bash macos_sign.sh microkit-bin-$name-${SDK_VERSION}"
echo "$cmd" > sign_cmd.sh
chmod +x sign_cmd.sh
scp sign_cmd.sh "$MAC_MINI_SSH":
read -p "Press Enter after running 'bash sign_cmd.sh' on the Mac"
rm sign_cmd.sh

for name in ${names[@]}; do
  if [[ "$name" =~ ^macos.* ]]; then
    scp "$MAC_MINI_SSH":"microkit-bin-$name-${SDK_VERSION}" "$name/microkit-sdk-${SDK_VERSION}/bin/microkit.signed"
    if diff "$name/microkit-sdk-${SDK_VERSION}/bin/microkit.signed" "$name/microkit-sdk-${SDK_VERSION}/bin/microkit"; then
      echo >&2 "Signed version not different :("
      exit 1
    fi
    mv "$name/microkit-sdk-${SDK_VERSION}/bin/microkit.signed" "$name/microkit-sdk-${SDK_VERSION}/bin/microkit"
    tar czf "microkit-sdk-${SDK_VERSION}-${name}.tar.gz" -C "$name/" "microkit-sdk-${SDK_VERSION}"
  fi
done

for name in ${names[@]}; do
  gpg --batch --yes -ab "microkit-sdk-${SDK_VERSION}-${name}.tar.gz"
done

popd
