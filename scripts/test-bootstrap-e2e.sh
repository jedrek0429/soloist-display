#!/bin/sh
# End-to-end bootstrap verification with a local, deterministic download fixture.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
temp=$(mktemp -d)
trap 'rm -rf "$temp"' EXIT HUP INT TERM
mkdir -p "$temp/bin" "$temp/assets"
cat > "$temp/assets/installer" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" > "$SOLOIST_TEST_INVOKED"
printf '%s\n' "${SOLOIST_DISPLAY_VERSION:-unset}" > "$SOLOIST_TEST_VERSION"
if [ -t 0 ]; then printf 'tty\n' > "$SOLOIST_TEST_STDIN"; else printf 'pipe\n' > "$SOLOIST_TEST_STDIN"; fi
EOF
chmod 755 "$temp/assets/installer"
hash=$(sha256sum "$temp/assets/installer" | cut -d' ' -f1)
for target in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf; do
  case "$target" in
    x86_64-*) legacy=x86_64-unknown-linux-musl ;;
    aarch64-*) legacy=aarch64-unknown-linux-musl ;;
    armv7-*) legacy=armv7-unknown-linux-musleabihf ;;
  esac
  for alias in "$target" "$legacy"; do
    printf '%s %s https://github.com/jedrek0429/soloist-display/releases/download/v0.0.0/soloist-display-%s\n' "$alias" "$hash" "$target"
  done
done > "$temp/manifest"
"$root/target/debug/release-tools" render-bootstrap "$root/install.sh" "$temp/manifest" \
  https://github.com/jedrek0429/soloist-display/releases/download/v0.0.0 > "$temp/bootstrap.sh"
cat > "$temp/bin/curl" <<'EOF'
#!/bin/sh
# Serve the fixture for any pinned release binary URL.
for arg do
  case "$arg" in
    -o) output_next=1 ;;
    *) if [ "${output_next:-0}" = 1 ]; then destination=$arg; output_next=0; fi ;;
  esac
done
cp "$SOLOIST_TEST_BINARY" "$destination"
EOF
chmod +x "$temp/bin/curl"
export SOLOIST_TEST_BINARY="$temp/assets/installer" SOLOIST_TEST_INVOKED="$temp/invoked" SOLOIST_TEST_VERSION="$temp/version" SOLOIST_TEST_STDIN="$temp/stdin"
PATH="$temp/bin:$PATH" sh "$temp/bootstrap.sh" --yes
grep -Fxq 'install --yes' "$temp/invoked"
grep -Fxq 'pipe' "$temp/stdin"
# A piped bootstrap must pass the controlling terminal to the manager.
command -v script >/dev/null 2>&1 || { echo 'util-linux script is required' >&2; exit 1; }
PATH="$temp/bin:$PATH" script -q -e -c "printf 'piped input\n' | sh '$temp/bootstrap.sh'" /dev/null </dev/null >"$temp/pty.log" 2>&1
grep -Fxq 'install' "$temp/invoked"
grep -Fxq 'tty' "$temp/stdin"
# A rendered alpha bootstrap must pin its matching signed release by default.
sed 's@/v0.0.0/@/v1.0.0-alpha.1/@g' "$temp/manifest" > "$temp/alpha-manifest"
printf '%s %s %s\n' x86_64-pc-windows-msvc "$hash" \
  'https://github.com/jedrek0429/soloist-display/releases/download/v1.0.0-alpha.1/soloist-display-windows-x86_64.exe' >> "$temp/alpha-manifest"
printf '%s %s %s\n' aarch64-apple-darwin "$hash" \
  'https://github.com/jedrek0429/soloist-display/releases/download/v1.0.0-alpha.1/soloist-display-macos-arm64' >> "$temp/alpha-manifest"
"$root/target/debug/release-tools" render-bootstrap "$root/install.ps1" "$temp/alpha-manifest" \
  https://github.com/jedrek0429/soloist-display/releases/download/v1.0.0-alpha.1 > "$temp/alpha.ps1"
grep -Fq "$hash" "$temp/alpha.ps1"
! grep -q '@SOLOIST_DISPLAY_' "$temp/alpha.ps1"
"$root/target/debug/release-tools" render-bootstrap "$root/install.sh" "$temp/alpha-manifest" \
  https://github.com/jedrek0429/soloist-display/releases/download/v1.0.0-alpha.1 > "$temp/alpha.sh"
sh -n "$temp/alpha.sh"
PATH="$temp/bin:$PATH" sh "$temp/alpha.sh" --yes
grep -Fxq '1.0.0-alpha.1' "$temp/version"
PATH="$temp/bin:$PATH" SOLOIST_DISPLAY_VERSION=1.0.0-alpha.1 sh "$temp/alpha.sh" --yes
grep -Fxq '1.0.0-alpha.1' "$temp/version"
# A tampered binary must be rejected before execution.
printf 'tamper\n' >> "$temp/assets/installer"
rm "$temp/invoked"
if PATH="$temp/bin:$PATH" sh "$temp/bootstrap.sh" --yes > "$temp/out" 2>&1; then
  echo 'Tampered fixture unexpectedly installed' >&2
  exit 1
fi
grep -q 'installer SHA-256 verification failed' "$temp/out"
test ! -e "$temp/invoked"
echo 'Bootstrap fixture verified: success and tamper rejection'
