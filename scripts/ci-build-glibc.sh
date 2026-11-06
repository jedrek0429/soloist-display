#!/usr/bin/env bash
# CI-only build environment for release-grade glibc Linux targets.
set -euo pipefail

target=${1:?usage: ci-build-glibc.sh TARGET}
case "$target" in
  x86_64-unknown-linux-gnu)
    test "$(uname -m)" = x86_64 || { echo 'x86_64 runner required' >&2; exit 1; }
    ;;
  aarch64-unknown-linux-gnu)
    test "$(uname -m)" = aarch64 || { echo 'ARM64 runner required' >&2; exit 1; }
    ;;
  armv7-unknown-linux-gnueabihf)
    sudo dpkg --add-architecture armhf
    sudo sed -i '/^Types: deb$/a Architectures: amd64' /etc/apt/sources.list.d/ubuntu.sources
    for suite in noble noble-updates noble-security; do
      echo "deb [arch=armhf signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] http://ports.ubuntu.com/ubuntu-ports $suite main universe"
    done | sudo tee /etc/apt/sources.list.d/armhf.list
    export PKG_CONFIG_ALLOW_CROSS=1
    export PKG_CONFIG_LIBDIR=/usr/lib/arm-linux-gnueabihf/pkgconfig
    export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc
    ;;
  *)
    echo "unsupported target: $target" >&2
    exit 1
    ;;
esac

sudo apt-get update
if [ "$target" = armv7-unknown-linux-gnueabihf ]; then
  sudo apt-get install -y gcc-arm-linux-gnueabihf pkg-config libasound2-dev:armhf qemu-user-static
else
  sudo apt-get install -y pkg-config libasound2-dev
fi

# Link both binaries against the declared glibc baseline.
for package in soloist-audio-bridge soloist-display-installer; do
  env -u CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER \
    cargo zigbuild --release --locked -p "$package" --target "$target.2.38"
done

for binary in soloist-display soloist-audio-bridge; do
  path="target/$target/release/$binary"
  file "$path"
  readelf -l "$path" | grep -q INTERP || { echo "$path is not a dynamic glibc ELF" >&2; exit 1; }
  readelf -d "$path" | grep NEEDED
  required=$(readelf --version-info "$path" | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sort -Vu | tail -1)
  echo "$path: highest required version is $required"
  if [ "$(printf '%s\n%s\n' "$required" GLIBC_2.38 | sort -Vu | tail -1)" != GLIBC_2.38 ]; then
    echo "$path requires glibc newer than 2.38" >&2
    exit 1
  fi
done

if [ "$target" = armv7-unknown-linux-gnueabihf ]; then
  readelf -h "target/$target/release/soloist-display" | grep -q 'hard-float ABI'
  readelf -h "target/$target/release/soloist-audio-bridge" | grep -q 'hard-float ABI'
  qemu-arm-static -L / "target/$target/release/soloist-display" --version
else
  ldd "target/$target/release/soloist-display" | tee /tmp/soloist-display-deps.txt
  ldd "target/$target/release/soloist-audio-bridge" | tee /tmp/soloist-audio-deps.txt
  ! grep -q 'not found' /tmp/soloist-display-deps.txt
  ! grep -q 'not found' /tmp/soloist-audio-deps.txt
  "target/$target/release/soloist-display" --version
fi
