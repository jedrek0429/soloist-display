#!/bin/sh
# Soloist Display bootstrap. Application installation is handled by the signed Rust manager.
set -eu

BOOTSTRAP_VERSION=1
PIN_RELEASE_BASE='@SOLOIST_DISPLAY_RELEASE_BASE@'
PIN_X86_64='@SOLOIST_DISPLAY_SHA_X86_64@'
PIN_AARCH64='@SOLOIST_DISPLAY_SHA_AARCH64@'
PIN_ARMV7='@SOLOIST_DISPLAY_SHA_ARMV7@'
RELEASE_BASE=${SOLOIST_DISPLAY_RELEASE_BASE:-$PIN_RELEASE_BASE}
[ "$RELEASE_BASE" != '@SOLOIST_DISPLAY_RELEASE_BASE@' ] || RELEASE_BASE=https://github.com/jedrek0429/soloist-display/releases/latest/download
tmp_dir=

say() { [ "${quiet:-0}" -eq 1 ] || printf '%s\n' "$*" >&2; }
fail() { printf 'soloist-display installer: %s\n' "$*" >&2; exit 1; }
cleanup() { [ -z "$tmp_dir" ] || rm -rf "$tmp_dir"; }
trap 'cleanup' EXIT HUP INT TERM

case "${1:-}" in
  -h|--help)
    cat <<'EOF'
Usage: install.sh [OPTIONS]

Downloads and verifies the Soloist Display installer, then runs it.

  --yes         accept safe defaults (required without a terminal)
  --no-color    disable coloured output
  --quiet       show only errors
  --verbose     show additional diagnostics
  --help        show this help
  --version     show bootstrap version
EOF
    exit 0
    ;;
  --version) printf 'soloist-display-bootstrap %s\n' "$BOOTSTRAP_VERSION"; exit 0 ;;
esac

[ "$(uname -s 2>/dev/null)" = Linux ] || fail 'Linux is required'
bits=$(getconf LONG_BIT 2>/dev/null || printf unknown)
machine=$(uname -m 2>/dev/null || printf unknown)
case "$machine:$bits" in
  x86_64:64|amd64:64) target=x86_64-unknown-linux-musl ;;
  aarch64:64|arm64:64) target=aarch64-unknown-linux-musl ;;
  armv7*:32|armv8*:32|armhf:32|aarch64:32) target=armv7-unknown-linux-musleabihf ;;
  *) fail "unsupported architecture or userland: $machine ($bits-bit)" ;;
esac

if command -v curl >/dev/null 2>&1; then
  fetch() { curl --fail --location --silent --show-error --retry 3 --retry-delay 1 --connect-timeout 10 --max-time 300 "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget --quiet --tries=3 --timeout=300 -O "$2" "$1"; }
else
  fail 'curl or wget is required'
fi
tmp_dir=$(mktemp -d "${TMPDIR:-/tmp}/soloist-display.XXXXXX") || fail 'cannot create a temporary directory'
manifest=$tmp_dir/bootstrap-manifest.txt
signature_b64=$tmp_dir/bootstrap-manifest.sig
signature=$tmp_dir/bootstrap-manifest.sig.bin
public_key=$tmp_dir/release-public.pem
installer=$tmp_dir/soloist-display

case "$target" in
  x86_64-*) pinned_sha=$PIN_X86_64 ;;
  aarch64-*) pinned_sha=$PIN_AARCH64 ;;
  armv7-*) pinned_sha=$PIN_ARMV7 ;;
esac

case "$pinned_sha" in
@*)
  command -v openssl >/dev/null 2>&1 || fail 'OpenSSL is required to authenticate installer metadata'

cat >"$public_key" <<'EOF'
-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAN2qrYATzPFCel3Nw7NZHzZtba1Gz9kthPRXWtQMFTiw=
-----END PUBLIC KEY-----
EOF

say "Downloading authenticated installer metadata..."
fetch "$RELEASE_BASE/bootstrap-manifest.txt" "$manifest" || fail 'could not download installer metadata'
fetch "$RELEASE_BASE/bootstrap-manifest.sig" "$signature_b64" || fail 'could not download installer signature'
openssl base64 -d -A -in "$signature_b64" -out "$signature" 2>/dev/null || fail 'invalid installer signature encoding'
openssl pkeyutl -verify -pubin -inkey "$public_key" -rawin -in "$manifest" -sigfile "$signature" >/dev/null 2>&1 || fail 'installer metadata signature verification failed'

record=$(awk -v target="$target" '$1 == target && $2 ~ /^[0-9a-f]{64}$/ && $3 ~ /^https:\/\// { print $2 " " $3; found++ } END { if (found != 1) exit 1 }' "$manifest") || fail "authenticated metadata has no unique installer for $target"
expected_sha=${record%% *}
installer_url=${record#* }
  ;;
*)
  installer_url="$RELEASE_BASE/soloist-display-$target"
  expected_sha=$pinned_sha
  ;;
esac
say "Downloading installer for $target..."
fetch "$installer_url" "$installer" || fail 'could not download the installer executable'
if command -v sha256sum >/dev/null 2>&1; then
  actual_sha=$(sha256sum "$installer" | awk '{print $1}')
elif command -v openssl >/dev/null 2>&1; then
  actual_sha=$(openssl dgst -sha256 "$installer" | awk '{print $NF}')
else
  fail 'sha256sum or OpenSSL is required to verify the installer'
fi
[ "$actual_sha" = "$expected_sha" ] || fail 'installer SHA-256 verification failed'
chmod 700 "$installer"

say 'Starting Soloist Display setup...'
if [ -r /dev/tty ] && [ -w /dev/tty ]; then
  "$installer" install "$@" </dev/tty
else
  "$installer" install "$@"
fi
