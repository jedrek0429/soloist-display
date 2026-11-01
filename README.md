# Soloist Display

**A customizable, full-screen now-playing display and real-time PipeWire audio visualizer for Spotify Soloist on Linux.** Built for dedicated music screens, CRT televisions, secondary monitors, and always-on album-art displays.

![Soloist Display showing the current track](docs/screenshots/now-playing.webp)

Soloist Display is an independent open-source frontend for **Spotify Soloist**. It reads playback metadata from Soloist's **local WebSocket API** and captures system audio through **PipeWire/PulseAudio** for an actual audio-reactive spectrum. It is not a Spicetify extension, Spotify Web API client, or standalone Spotify Connect receiver: **Soloist provides playback and Spotify Connect; this project provides the display.**

## Features

- **Live now playing:** album artwork, song title, artist, playback state, progress bar, elapsed/remaining/total time; reconnects automatically when Soloist or the audio stream disconnects.
- **Animated disc artwork:** basic disc, CD and vinyl styles; adjustable disc size, centre hole, colour and rotation speed; independent fly, slide, fade or disabled disc transitions.
- **Album-art backdrop:** blurred, dimmed artwork with separate fade transition and duration controls.
- **Real-time audio spectrum:** local Rust FFT audio bridge with six frequency bands; sensitivity, smoothing, automatic normalization and peak-hold controls. Uses audio capture rather than simulated playback motion.
- **Customizable metadata panel:** four screen corners, opacity, text scale, panel scale, progress bar and time display; independently configurable panel transitions.
- **Display-friendly controls:** reduced motion, burn-in protection, independently configurable animation timings and automatic playback pause handling.
- **Built-in settings UI:** edit settings in your browser, save locally, reset to defaults, and automatically refresh the display after configuration changes.
- **Local-first operation:** HTTP UI on `127.0.0.1:8080`, Soloist events on `127.0.0.1:9090`, spectrum WebSocket on `127.0.0.1:9091` by default. No separate cloud service for the display.
- **Linux desktop integration:** `soloist-display` CLI (`--settings`, `--help`, `--version`), application launchers, systemd user service, and an Arch Linux `-git` PKGBUILD. Arch packaging also installs a man page and zsh completion.

## Compatibility

| Platform | Installation | Status |
| --- | --- | --- |
| Arch Linux / Arch-based | `makepkg -si` | CI-tested package build and install |
| Ubuntu / Debian | User-local source installer | Supported path; not yet distribution-tested |
| Fedora | User-local source installer | Supported path; not yet distribution-tested |
| Other systemd Linux distributions | User-local source installer | May work with dependencies installed |
| macOS / Windows | — | Not supported (PipeWire/PulseAudio and systemd integration required) |

**Requirements:** Linux with a graphical desktop/browser, a systemd user session, PipeWire with PulseAudio compatibility (`pactl`), Rust/Cargo for source installation, `curl`, `xdg-open`, Git, and **Spotify Soloist** running locally. Spotify Soloist requires Spotify Premium and its own API key. Currently the documented official Soloist Linux download is x86_64; do not assume other architectures are supported.

## Install

### 1. Install system dependencies

**Arch Linux:**

```sh
sudo pacman -Syu
sudo pacman -S --needed base-devel git curl xdg-utils pipewire pipewire-pulse wireplumber libpulse
```

**Ubuntu / Debian:**

```sh
sudo apt update
sudo apt install -y git curl xdg-utils build-essential pkg-config libpulse0 pulseaudio-utils pipewire pipewire-pulse wireplumber cargo
```

**Fedora:**

```sh
sudo dnf install -y git curl xdg-utils gcc make pkgconf-pkg-config pulseaudio-libs pulseaudio-utils pipewire pipewire-pulseaudio wireplumber cargo
```

Distribution package names and service setup can vary by release. Ensure `pactl info` works in your desktop session. An up-to-date Rust toolchain may be necessary if your distribution's Cargo is older.

### 2. Set up Spotify Soloist (skip if already running)

Download the official [Spotify Soloist](https://developer.spotify.com/documentation/soloist) Linux build and obtain your private key from the [Spotify for Developers dashboard](https://developer.spotify.com/dashboard/soloist). Do not commit or share the key.

```sh
mkdir -p "$HOME/Downloads/soloist" "$HOME/.local/bin" "$HOME/.config/systemd/user" "$HOME/.config/soloist"
cd "$HOME/Downloads/soloist"
curl --fail --location -o soloist.tar.gz https://soloist-builds.spotifycdn.com/soloist_release_x86_64.tar.gz
tar -xzf soloist.tar.gz
install -Dm755 soloist "$HOME/.local/bin/soloist"
git clone https://github.com/jedrek0429/soloist-display.git "$HOME/soloist-display"
install -Dm644 "$HOME/soloist-display/packaging/soloist.service.example" "$HOME/.config/systemd/user/soloist.service"
${EDITOR:-nano} "$HOME/.config/soloist/env"
```

Add to `~/.config/soloist/env`:

```ini
SOLOIST_NAME="Soloist Display"
SOLOIST_API_KEY="YOUR_PRIVATE_API_KEY"
```

Protect the key and start Soloist:

```sh
chmod 600 "$HOME/.config/soloist/env"
systemctl --user daemon-reload
systemctl --user enable --now soloist.service
```

Select the device in Spotify Connect. Soloist Display expects Soloist's WebSocket at `127.0.0.1:9090`.

### 3. Install Soloist Display

If you skipped Step 2, clone the repository now:

```sh
git clone https://github.com/jedrek0429/soloist-display.git "$HOME/soloist-display"
cd "$HOME/soloist-display"
```

**Arch Linux (native package):**

```sh
cd "$HOME/soloist-display"
makepkg -si
```

**Ubuntu, Debian, Fedora, other systemd Linux (user-local installation):**

```sh
cd "$HOME/soloist-display"
bash packaging/install-linux.sh
```

The source installer compiles the Rust audio bridge and installs files under `~/.local`; it does not install a `.deb` or `.rpm` package. It does not require `sudo`. Ensure `~/.local/bin` is on your `PATH`.

Launch with `soloist-display` or `~/.local/bin/soloist-display`, or choose **Soloist Display** from your desktop app menu. Open settings with `soloist-display --settings`. Settings persist in `~/.config/soloist-display/config.json`.

## Architecture

```text
Spotify app --Spotify Connect--> Spotify Soloist
                                  |
                                  +-- local playback WebSocket :9090 --> browser display
PipeWire / PulseAudio --> Rust FFT audio bridge :9091 --> browser spectrum
                                  |
                                  +-- local HTTP UI :8080
```

The Rust bridge embeds the web UI, so no Node.js server or second terminal is needed after installation. The launcher starts the systemd user service and opens the browser. The display does not download music or replace Spotify Soloist.

## Configuration and troubleshooting

Run `soloist-display --settings` to configure visuals and connections. For service diagnostics:

```sh
systemctl --user status soloist soloist-display
journalctl --user -u soloist -u soloist-display -n 50
pactl get-default-sink
```

For Arch package help, run `man soloist-display`. Soloist builds may expire; replace the Soloist binary with a current [official build](https://developer.spotify.com/documentation/soloist/reference/downloads-and-updates).

## Uninstall

**Arch:** `systemctl --user disable --now soloist-display.service && sudo pacman -Rns soloist-display-git`.

**User-local install:**

```sh
systemctl --user disable --now soloist-display.service 2>/dev/null || true
rm -f "$HOME/.local/bin/soloist-display" "$HOME/.local/bin/soloist-audio-bridge"
rm -f "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/soloist-display.service"
rm -f "${XDG_DATA_HOME:-$HOME/.local/share}/applications/soloist-display.desktop"
rm -f "${XDG_DATA_HOME:-$HOME/.local/share}/applications/soloist-display-settings.desktop"
systemctl --user daemon-reload
```

This leaves Spotify Soloist and saved display settings intact.

## License

MIT. See [LICENSE](LICENSE).
