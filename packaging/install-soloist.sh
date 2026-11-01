#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != Linux ]]; then
  echo "Spotify Soloist downloads in this script support Linux only." >&2
  exit 1
fi

machine="${SOLOIST_TEST_MACHINE:-$(uname -m)}"
bits="${SOLOIST_TEST_BITS:-$(getconf LONG_BIT)}"
case "$machine:$bits" in
  x86_64:64|amd64:64) archive=x86_64 ;;
  aarch64:64|arm64:64) archive=arm64 ;;
  armv7*:32|armv8*:32|armhf:32|aarch64:32) archive=arm32 ;;
  *) printf 'Unsupported Linux architecture: %s (%s-bit userspace)\n' "$machine" "$bits" >&2; exit 1 ;;
esac

if [[ "${1:-}" == "--print-archive" ]]; then
  printf "%s\n" "$archive"
  exit 0
fi
url="https://soloist-builds.spotifycdn.com/soloist_release_${archive}.tar.gz"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
printf 'Downloading Spotify Soloist for %s...\n' "$archive"
curl --fail --location --show-error --silent "$url" -o "$tmp/soloist.tar.gz"
# Extract only the executable, not other archive members.
tar -xzf "$tmp/soloist.tar.gz" -C "$tmp" --no-same-owner --no-same-permissions soloist
test -f "$tmp/soloist" || { echo 'Soloist executable missing from archive.' >&2; exit 1; }
install -Dm755 "$tmp/soloist" "$HOME/.local/bin/soloist"
printf 'Installed %s\n' "$HOME/.local/bin/soloist"
