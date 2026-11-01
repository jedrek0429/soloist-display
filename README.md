# Soloist Display

Minimal Spotify now-playing display for a CRT. Album art, live spectrum, track progress.

**Arch Linux · Firefox · Spotify Soloist**

> Already running Soloist with `--ws 127.0.0.1:9090`? **Skip to [3. Install Display](#3-install-display).**

## 1. Prepare Arch

```sh
sudo pacman -S --needed base-devel git firefox pipewire pipewire-pulse libpulse curl
```

You need **Spotify Premium** to pair Soloist and a **Soloist API key** from [Spotify for Developers](https://developer.spotify.com/documentation/soloist).

## 2. Set up Spotify Soloist

1. Download **Linux x86_64** from [official Soloist downloads](https://developer.spotify.com/documentation/soloist/reference/downloads-and-updates).
2. Extract the archive. Install its `soloist` executable:

```sh
mkdir -p ~/.local/bin ~/.config/soloist ~/.config/systemd/user
install -m 755 /path/to/extracted/soloist ~/.local/bin/soloist
```

3. Create `~/.config/soloist/env`:

```ini
SOLOIST_NAME=ThinkCentre CRT
SOLOIST_API_KEY=PASTE_YOUR_KEY_HERE
```

```sh
chmod 600 ~/.config/soloist/env
```

4. Install the background service:

```sh
git clone https://github.com/jedrek0429/soloist-display.git
cd soloist-display
git switch feat/now-playing-display
cp packaging/soloist.service.example ~/.config/systemd/user/soloist.service
systemctl --user daemon-reload
systemctl --user enable --now soloist.service
```

Open Spotify on your phone → **Connect to a device** → **ThinkCentre CRT**. Play something. Spotify Soloist must expose port **9090**.

## 3. Install Display

If you completed step 2, stay in the repository directory. Otherwise:

```sh
git clone https://github.com/jedrek0429/soloist-display.git
cd soloist-display
git switch feat/now-playing-display
```

Install:

```sh
makepkg -si
```

Open **Rofi → Soloist Display**. For appearance controls, open **Rofi → Soloist Display Settings**.

No terminal needs to stay open. The display service starts when launched.

## Settings

Sliders for disc, background, transitions, metadata and live audio spectrum. **Save** applies changes to the display; **Reset** restores defaults.

Settings file: `~/.config/soloist-display/config.json`.

## Troubleshooting

```sh
systemctl --user status soloist
systemctl --user status soloist-display
journalctl --user -u soloist-display -n 40
```

No music metadata? Check Soloist is running with `--ws 127.0.0.1:9090`. No spectrum? Check the default audio sink with `pactl get-default-sink`.

**Note:** Soloist builds expire after 90 days; replace the executable with a newer official build. [Spotify documentation](https://developer.spotify.com/documentation/soloist).
