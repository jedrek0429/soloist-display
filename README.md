# Soloist Display

A full-screen now-playing display for [Spotify Soloist](https://developer.spotify.com/documentation/soloist).

![Soloist Display](docs/screenshots/now-playing.webp)

Pick music on your phone. See the artwork, track details and live audio spectrum on your Linux computer or Raspberry Pi.

## Install

Requires Spotify Premium, systemd, glibc 2.38+ and Linux on x86_64, ARM64 or hard-float ARMv7 (Ubuntu 24.04+ or Debian 13+, for example).

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

## Uninstall

```sh
soloist-display uninstall
```

Choose what happens to Soloist and whether to keep Display settings. The choices depend on what was installed. No mystery `--mode` flags.

See [advanced installation](docs/advanced-installation.md) for automation and exact uninstall options, [troubleshooting](docs/troubleshooting.md) for faults, and [installer details](docs/installer.md) for what gets changed.

MIT
