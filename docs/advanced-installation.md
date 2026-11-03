# Advanced installation

The [README](../README.md) covers a normal installation. Use the commands below only when you need to choose a release or automate setup.

## Prereleases and specific versions

The standard installation command chooses the latest stable release when one exists, or the latest prerelease before the first stable release.

To install a particular release, specify its version:

```sh
curl -fsSL https://jedrek0429.github.io/soloist-display/install.sh | SOLOIST_DISPLAY_VERSION=1.0.0-alpha.1 sh
```

For an already installed version, the CLI supports choosing a channel or version:

```sh
soloist-display check-updates
soloist-display update --channel prerelease
soloist-display update --channel stable
soloist-display update --version 1.2.0-rc.3
```

The selected channel is remembered. On the prerelease channel, the next stable release is also eligible for an update. Updates require your confirmation when prompted or an explicit `update` command. Disable or re-enable update notices with `soloist-display notifications off` or `soloist-display notifications on`.

A downgrade may be incompatible with newer settings. Back up your settings first, then opt in explicitly:

```sh
soloist-display update --version 1.0.0-alpha.1 --allow-downgrade
```

## Unattended setup

The installer requires `--yes` when no terminal is attached. Set the Spotify Connect device name and Soloist API key in the environment:

```sh
SOLOIST_NAME="Living Room" SOLOIST_API_KEY="YOUR_KEY" sh -c 'curl -fsSL https://jedrek0429.github.io/soloist-display/install.sh | sh -s -- --yes'
```

Avoid putting a real API key directly into shell history or committed scripts. For interactive Bash, you can read the key without echoing it:

```bash
export SOLOIST_NAME="Living Room"
read -rsp "Soloist API key: " SOLOIST_API_KEY; export SOLOIST_API_KEY; echo
curl -fsSL https://jedrek0429.github.io/soloist-display/install.sh | sh -s -- --yes
unset SOLOIST_API_KEY
```

## Platform and security

Requires Spotify Premium, a systemd-based Linux installation, glibc 2.38 or newer, and x86_64, ARM64 or hard-float ARMv7. The manager verifies signed release manifests and artifact checksums. An independently installed Spotify Soloist is preserved.

For details about managed files, rollback, services and troubleshooting, see [installer architecture](installer.md). Maintainers should use the [release guide](releasing.md).
