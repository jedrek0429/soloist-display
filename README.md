# Soloist Display

[![Release](https://img.shields.io/github/v/release/jedrek0429/soloist-display?include_prereleases&style=for-the-badge)](https://github.com/jedrek0429/soloist-display/releases)
[![Checks](https://img.shields.io/github/actions/workflow/status/jedrek0429/soloist-display/build.yml?branch=main&style=for-the-badge&label=Checks)](https://github.com/jedrek0429/soloist-display/actions/workflows/build.yml)
[![Installer integration](https://img.shields.io/github/actions/workflow/status/jedrek0429/soloist-display/installer-integration.yml?branch=main&style=for-the-badge&label=Installer)](https://github.com/jedrek0429/soloist-display/actions/workflows/installer-integration.yml)
[![License: MIT](https://img.shields.io/github/license/jedrek0429/soloist-display?style=for-the-badge)](LICENSE)
[![Code quality](https://img.shields.io/codefactor/grade/github/jedrek0429/soloist-display/main?style=for-the-badge&label=Code%20quality)](https://www.codefactor.io/repository/github/jedrek0429/soloist-display)

A full-screen now-playing display for [Spotify Soloist](https://developer.spotify.com/documentation/soloist).

![Soloist Display](docs/screenshots/now-playing.webp)

Pick music on your phone. See the artwork, track details and live audio spectrum on your Linux computer or Raspberry Pi.

## Install

Requires **Spotify Premium**, systemd, glibc 2.38+ and Linux on x86_64, ARM64 or hard-float ARMv7 (Ubuntu 24.04+ or Debian 13+, for example).

```sh
curl -fsSL https://jedrek0429.github.io/soloist-display/install.sh | sh
```

Follow the prompts. Have your [Soloist API key](https://developer.spotify.com/dashboard/soloist) ready.

Already running Soloist? Choose **Managed** (Display maintains it) or **External** (you maintain it).

## Use

```sh
soloist-display                  # open the display
soloist-display settings         # change the look
soloist-display update           # update Display
soloist-display doctor           # check what's wrong
```

Managed Soloist checks for new builds every six hours. External Soloist is left alone.

## Media keys (optional)

Enable MPRIS support for media keys and `playerctl` with `soloist-display mpris enable`. See [MPRIS setup](docs/advanced-installation.md#optional-mpris-bridge).

Thanks to [@TobiasVeiga00](https://github.com/TobiasVeiga00) for creating [soloist-mpris](https://github.com/TobiasVeiga00/soloist-mpris), the bridge behind this integration.

## Uninstall

```sh
soloist-display uninstall
```

Choose what happens to Soloist and whether to keep Display settings. The options depend on what was installed.

See [advanced installation](docs/advanced-installation.md) for automation and exact uninstall options, [troubleshooting](docs/troubleshooting.md) for faults, and [installer details](docs/installer.md) for what gets changed.

MIT
