# Soloist Display

A full-screen now-playing display and live audio visualizer for [Spotify Soloist](https://developer.spotify.com/documentation/soloist).

![Soloist Display](docs/screenshots/now-playing.webp)

Turn your Linux computer or Raspberry Pi into a dedicated Spotify speaker with a beautiful display.

Choose a song on your phone, select your computer in Spotify Connect, and enjoy your music with animated album artwork, track information, and a live audio spectrum.

## Install

Requires Spotify Premium and a systemd-based Linux installation with glibc 2.38 or newer (including Ubuntu 24.04+ and Debian 13+) on x86_64, ARM64, or hard-float ARMv7.

```sh
curl -fsSL https://jedrek0429.github.io/soloist-display/install.sh | sh
```

To select a signed release, set `SOLOIST_DISPLAY_VERSION` (defaults to `latest`):

```sh
curl -fsSL https://jedrek0429.github.io/soloist-display/install.sh | SOLOIST_DISPLAY_VERSION=0.2.0 sh
```

The installer verifies signed release files, installs required runtime packages, and configures user services. If Soloist is already working on `127.0.0.1:9090`, it is preserved and reused. Otherwise, have a [Soloist API key](https://developer.spotify.com/dashboard/soloist) ready; setup asks only for the Spotify Connect device name and API key.

Open a new terminal after the first installation if `soloist-display` is not yet on your `PATH`.

For unattended setup, provide the same values without putting the secret in command history:

```sh
export SOLOIST_NAME="Soloist Display"
read -rsp "Soloist API key: " SOLOIST_API_KEY; export SOLOIST_API_KEY; echo
curl -fsSL https://jedrek0429.github.io/soloist-display/install.sh | sh -s -- --yes
unset SOLOIST_API_KEY
```

## Use

```sh
soloist-display             # open the display
soloist-display settings    # open settings
soloist-display update      # install a verified update
soloist-display repair      # restore managed files and services
soloist-display doctor      # check services, ports, Soloist and audio
```

The display checks briefly for updates when opened and offers to install one when available.

## Remove

```sh
soloist-display uninstall --mode display  # keep settings and Soloist
soloist-display uninstall --mode data     # also remove display settings and data
soloist-display uninstall --mode all      # also remove Soloist, only when installer-owned
```

An independently installed Soloist is never removed. Runtime packages installed through the system package manager are left in place because they may be used by other applications.

## Troubleshooting

Run `soloist-display doctor`, then inspect `journalctl --user -u soloist-display -u soloist` if a service fails. Soloist builds expire after 90 days; `soloist-display update` installs the current verified build.

[Installer architecture and recovery](docs/installer.md) · [Release signing](docs/releasing.md) · MIT
