#!/usr/bin/env bash
set -euo pipefail

# User-local installer for systemd-based Linux distributions.
for tool in cargo rustc pactl systemctl curl xdg-open install sed; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    printf 'Missing requirement: %s\n' "$tool" >&2
    exit 1
  fi
done
if [[ "$(uname -s)" != Linux ]]; then
  echo 'Soloist Display currently supports Linux only.' >&2
  exit 1
fi
if ! systemctl --user show-environment >/dev/null 2>&1; then
  echo "A working systemd user session is required. Log into a graphical or user systemd session and retry." >&2
  exit 1
fi
if ! command -v soloist >/dev/null 2>&1 && [[ ! -x "$HOME/.local/bin/soloist" ]]; then
  echo "Spotify Soloist is not installed. Run: bash packaging/install-soloist.sh" >&2
  exit 1
fi
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
cargo build --release --manifest-path audio-bridge/Cargo.toml
bin_dir="${HOME}/.local/bin"
service_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
app_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$bin_dir" "$service_dir" "$app_dir"
install -m755 audio-bridge/target/release/soloist-audio-bridge "$bin_dir/soloist-audio-bridge"
install -m755 packaging/soloist-display-launcher "$bin_dir/soloist-display"
revision="$(git rev-list --count HEAD 2>/dev/null || echo 0)"
hash="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
sed -i "s/@SOLOIST_DISPLAY_VERSION@/r${revision}.g${hash}/g" "$bin_dir/soloist-display"
sed "s|/usr/bin/soloist-audio-bridge|${bin_dir}/soloist-audio-bridge|" packaging/soloist-display.service > "$service_dir/soloist-display.service"
install -m644 packaging/soloist-display.desktop "$app_dir/soloist-display.desktop"
install -m644 packaging/soloist-display-settings.desktop "$app_dir/soloist-display-settings.desktop"
systemctl --user daemon-reload
printf 'Installed to %s. Start with: %s/soloist-display\n' "$bin_dir" "$bin_dir"
printf 'Soloist must already be running with WebSocket on 127.0.0.1:9090.\n'
