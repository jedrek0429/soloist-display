# Soloist Display

Turn a Linux computer into a Spotify Connect player with a simple, full-screen now-playing display. Pick music on your phone. The computer plays it and shows album art, track info and a live audio spectrum.

The screen is for display, not playback controls. Good for a spare monitor or a dedicated music setup.

![Soloist Display](docs/screenshots/now-playing.webp)

It uses [Spotify Soloist](https://developer.spotify.com/documentation/soloist) for playback and Spotify Connect. Soloist Display adds the screen. You install both.

## Features

- Album art, artist, track, progress and time.
- Spinning CD, vinyl or plain disc, with adjustable size and speed.
- Fly, slide and fade transitions. Separate backdrop and text animations.
- Blurred album-art background with adjustable darkness.
- Six-band FFT spectrum from PipeWire audio. Sensitivity, smoothing, auto-normalization and peak hold.
- Movable text panel, font size, opacity and progress bar.
- Reduced motion and burn-in protection.
- Browser settings saved locally. Changes appear without restarting the service.
- Automatic WebSocket reconnection.
- CLI, desktop shortcuts and a systemd user service.

## Install

Requires Linux, systemd, PipeWire/PulseAudio, a browser and Spotify Premium. You'll also install Spotify Soloist, the playback service, and get its API key.

### Arch Linux

```sh
sudo pacman -S --needed base-devel git curl xdg-utils pipewire pipewire-pulse wireplumber libpulse
git clone https://github.com/jedrek0429/soloist-display.git
cd soloist-display
makepkg -si
```

### Ubuntu / Debian

```sh
sudo apt update
sudo apt install -y git curl xdg-utils build-essential pkg-config libpulse0 pulseaudio-utils pipewire pipewire-pulse wireplumber cargo
git clone https://github.com/jedrek0429/soloist-display.git
cd soloist-display
bash packaging/install-linux.sh
```

### Fedora

```sh
sudo dnf install -y git curl xdg-utils gcc make pkgconf-pkg-config pulseaudio-libs pulseaudio-utils pipewire pipewire-pulseaudio wireplumber cargo
git clone https://github.com/jedrek0429/soloist-display.git
cd soloist-display
bash packaging/install-linux.sh
```

Arch x86_64 packaging is CI-tested. ARM64 and ARMv7 builds are enabled but not yet hardware-tested. The Debian and Fedora source installers are not yet tested on those distributions. Other systemd-based Linux distributions may work. Windows and macOS are not supported.

### Set up Spotify Connect playback

Spotify Soloist makes the computer appear as a Spotify Connect device. Skip this step if you already have it running with a WebSocket on `127.0.0.1:9090`.

Get an API key from the [Spotify developer dashboard](https://developer.spotify.com/dashboard/soloist). Install a Soloist build for your CPU architecture. The URL below is for x86_64; on ARM64 or ARMv7, get the matching Linux build URL from Spotify's downloads page and set `SOLOIST_DOWNLOAD_URL` before running the commands:

```sh
mkdir -p "$HOME/Downloads/soloist" "$HOME/.local/bin" "$HOME/.config/systemd/user" "$HOME/.config/soloist"
cd "$HOME/Downloads/soloist"
curl -fL -o soloist.tar.gz "${SOLOIST_DOWNLOAD_URL:-https://soloist-builds.spotifycdn.com/soloist_release_x86_64.tar.gz}"
tar -xzf soloist.tar.gz
install -Dm755 soloist "$HOME/.local/bin/soloist"
install -Dm644 "$HOME/soloist-display/packaging/soloist.service.example" "$HOME/.config/systemd/user/soloist.service"
${EDITOR:-nano} "$HOME/.config/soloist/env"
```

Add:

```ini
SOLOIST_NAME="Soloist Display"
SOLOIST_API_KEY="YOUR_PRIVATE_API_KEY"
```

Then:

```sh
chmod 600 "$HOME/.config/soloist/env"
systemctl --user daemon-reload
systemctl --user enable --now soloist.service
```

Choose the device in Spotify Connect.

## Run

```sh
soloist-display
soloist-display --settings
```

If the command isn't on your PATH, use `~/.local/bin/soloist-display`.

The display runs at `http://127.0.0.1:8080`. Soloist uses WebSocket port 9090; the audio bridge uses 9091. Settings are in `~/.config/soloist-display/config.json`.

## Problems

```sh
systemctl --user status soloist soloist-display
journalctl --user -u soloist -u soloist-display -n 50
pactl get-default-sink
```

Soloist builds expire. Get a current one from [Spotify](https://developer.spotify.com/documentation/soloist/reference/downloads-and-updates).

## Remove

Arch:

```sh
systemctl --user disable --now soloist-display.service
sudo pacman -Rns soloist-display-git
```

Other Linux:

```sh
systemctl --user disable --now soloist-display.service 2>/dev/null || true
rm -f "$HOME/.local/bin/soloist-display" "$HOME/.local/bin/soloist-audio-bridge"
rm -f "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/soloist-display.service"
rm -f "${XDG_DATA_HOME:-$HOME/.local/share}/applications/soloist-display.desktop"
rm -f "${XDG_DATA_HOME:-$HOME/.local/share}/applications/soloist-display-settings.desktop"
systemctl --user daemon-reload
```

This keeps your settings and Soloist installation.

MIT license.
