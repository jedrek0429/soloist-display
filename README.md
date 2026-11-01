# Soloist Display

![Soloist Display showing the current track](docs/screenshots/now-playing.webp)

A minimal now-playing screen for [Spotify Soloist](https://developer.spotify.com/documentation/soloist), with album art, track progress and a live audio spectrum.

> **Already have Soloist running? Skip to [Step 3](#3-install-soloist-display).**

## 1. Prepare Arch Linux

From a fresh installation:

```sh
sudo pacman -Syu
sudo pacman -S --needed base-devel git curl xdg-utils pipewire pipewire-pulse wireplumber libpulse
```

## 2. Install Spotify Soloist

Soloist needs Spotify Premium and a private API key. Generate yours in the [Spotify for Developers dashboard](https://developer.spotify.com/dashboard/soloist), then install the official x86_64 Linux build:

```sh
mkdir -p "$HOME/Downloads/soloist"
cd "$HOME/Downloads/soloist"
curl --fail --location -o soloist.tar.gz https://soloist-builds.spotifycdn.com/soloist_release_x86_64.tar.gz
tar -xzf soloist.tar.gz
install -Dm755 soloist "$HOME/.local/bin/soloist"
"$HOME/.local/bin/soloist" --version
```

Clone this repository and install its Soloist user service:

```sh
git clone https://github.com/jedrek0429/soloist-display.git "$HOME/soloist-display"
install -Dm644 "$HOME/soloist-display/packaging/soloist.service.example" "$HOME/.config/systemd/user/soloist.service"
install -Dm600 /dev/null "$HOME/.config/soloist/env"
```

Open `~/.config/soloist/env` in a text editor and add your device name and key:

```ini
SOLOIST_NAME="Soloist Display"
SOLOIST_API_KEY="PASTE_YOUR_KEY_HERE"
```

Start Soloist:

```sh
systemctl --user daemon-reload
systemctl --user enable --now soloist.service
```

In Spotify, open **Connect to a device** and choose the value of `SOLOIST_NAME`. Soloist Display expects Soloist's WebSocket at `127.0.0.1:9090`.

## 3. Install Soloist Display

If you skipped Step 2, clone the repository first:

```sh
git clone https://github.com/jedrek0429/soloist-display.git "$HOME/soloist-display"
```

Build and install:

```sh
cd "$HOME/soloist-display"
makepkg -si
```

Choose **Soloist Display** from your desktop application menu, or run `soloist-display`. Open **Soloist Display Settings** or run `soloist-display --settings` to adjust its appearance.

<img src="docs/screenshots/settings.webp" alt="Soloist Display settings" width="520">

Settings are stored in `~/.config/soloist-display/config.json`.

## Future AUR publication (maintainer)

The `PKGBUILD` in this repository is prepared for a future `soloist-display-git` AUR submission. No AUR account or publication is needed to build locally. Once you have an AUR account and SSH access, create the AUR repository and copy `PKGBUILD` into it, then run:

```sh
makepkg --printsrcinfo > .SRCINFO
makepkg -si
git add PKGBUILD .SRCINFO
git commit -m 'Initial AUR release'
git push
```

Before publishing, verify the upstream URL, licence, runtime dependencies and that the main branch builds. Maintain the AUR `PKGBUILD` and `.SRCINFO` together after subsequent packaging changes. `yay -Syu --devel` checks development packages for upstream revisions; ordinary `pacman -Syu` does not update AUR packages.

## Uninstall

Remove Soloist Display while keeping Spotify Soloist and your saved settings:

```sh
systemctl --user disable --now soloist-display.service
sudo pacman -Rns soloist-display-git
systemctl --user daemon-reload
```

For a complete cleanup, the following also deletes your settings, Soloist API key, Soloist binary, downloads and repository checkout:

```sh
systemctl --user disable --now soloist.service
rm -f -- "$HOME/.config/systemd/user/soloist.service" "$HOME/.local/bin/soloist"
rm -rf -- "$HOME/.config/soloist" "$HOME/.config/soloist-display" "$HOME/Downloads/soloist" "$HOME/soloist-display"
systemctl --user daemon-reload
```

## Troubleshooting

```sh
systemctl --user status soloist soloist-display
journalctl --user -u soloist -u soloist-display -n 50
pactl get-default-sink
```

Soloist builds expire after 90 days; replace the binary with a current [official download](https://developer.spotify.com/documentation/soloist/reference/downloads-and-updates).
