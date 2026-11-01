# Soloist Display

A now-playing display for Spotify Soloist on Linux. Album art, animated CDs and vinyl, track info and a live PipeWire audio spectrum. Works well on a CRT or second monitor.

![Soloist Display](docs/screenshots/now-playing.webp)

Soloist handles Spotify Connect and playback. This app connects to its local WebSocket and displays what's playing. It doesn't use the Spotify Web API.

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

Requires Linux, systemd, PipeWire/PulseAudio, a browser and [Spotify Soloist](https://developer.spotify.com/documentation/soloist) running locally. Soloist requires Spotify Premium and an API key.

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

Arch packaging is CI-tested. The Debian and Fedora source installers are not yet tested on those distributions. Other systemd-based Linux distributions may work. Windows and macOS are not supported.

### Set up Soloist

Skip this if Soloist already works and exposes its WebSocket on `127.0.0.1:9090`.

Get an API key from the [Spotify developer dashboard](https://developer.spotify.com/dashboard/soloist). Install the official x86_64 Linux build:

```sh
mkdir -p "$HOME/Downloads/soloist" "$HOME/.local/bin" "$HOME/.config/systemd/user" "$HOME/.config/soloist"
cd "$HOME/Downloads/soloist"
curl -fL -o soloist.tar.gz https://soloist-builds.spotifycdn.com/soloist_release_x86_64.tar.gz
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
