# Soloist Display

A full-screen now-playing display and live audio visualizer for [Spotify Soloist](https://developer.spotify.com/documentation/soloist).

![Soloist Display](docs/screenshots/now-playing.webp)

Turn your Linux computer or Raspberry Pi into a dedicated Spotify speaker with a beautiful display.

Choose a song on your phone, select your computer in Spotify Connect, and enjoy animated album artwork, track information, and a live audio spectrum.

## Install

Requires Spotify Premium and Linux with systemd and glibc 2.38 or newer (e.g. Ubuntu 24.04 or Debian 13). Supports x86_64, ARM64 and ARMv7.

```sh
curl -fsSL https://jedrek0429.github.io/soloist-display/install.sh | sh
```

Follow the setup prompts. Have your [Soloist API key](https://developer.spotify.com/dashboard/soloist) ready. If the command is not found after installation, open a new terminal.

## Use

```sh
soloist-display
```

To change the appearance, run `soloist-display settings`. To update, run `soloist-display update`.

## Uninstall

```sh
soloist-display uninstall --mode display
```

This keeps your settings and Spotify Soloist. To remove display settings too, use `--mode data`. To also remove Soloist when it was installed by Soloist Display, use `--mode all`.

For [prerelease or specific-version installations](docs/advanced-installation.md), [unattended setup](docs/advanced-installation.md#unattended-setup), or [troubleshooting](docs/installer.md), see the guides.

MIT
