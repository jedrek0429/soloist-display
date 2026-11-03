# Advanced installation

The [README](../README.md) covers normal use.

## Pick a release

The bootstrap selects the latest stable release, or the latest prerelease if there is no stable release yet.

```sh
curl -fsSL https://jedrek0429.github.io/soloist-display/install.sh | SOLOIST_DISPLAY_VERSION=1.0.0-alpha.1 sh
```

After installation:

```sh
soloist-display check-updates
soloist-display update --channel stable
soloist-display update --channel prerelease
soloist-display update --version 1.2.0-rc.3
soloist-display update --version 1.0.0-alpha.1 --allow-downgrade
soloist-display notifications off
```

The channel is remembered. Downgrades need explicit consent; older settings may not survive them. `notifications on` restores update prompts.

## Managed or External?

**Managed** is the default. Display installs and maintains a Soloist binary and user service. It checks the official Spotify CDN every six hours. To check now:

```sh
soloist-display renew-soloist
soloist-display renew-soloist --force
```

`--force` bypasses the unchanged-build shortcut. It does not bypass validation.

**External** uses your existing Soloist. Display does not replace its binary, service or data:

```sh
soloist-display install --external-soloist --soloist-ws ws://127.0.0.1:9090/
```

The WebSocket must respond to Soloist's protocol. If the service is stopped, interactive setup asks before starting it. For unattended setup, `--start-external` gives explicit permission to start it.

For a supported existing user service, Managed saves its unit and clones its session. The original stays put. Unsupported systemd settings, drop-ins and startup flags stop migration. Guesswork is not a migration strategy.

## Unattended setup

Use `--yes` without a terminal. For Managed, supply the Soloist API key through the environment, not your shell history:

```bash
export SOLOIST_NAME="Living Room"
read -rsp "Soloist API key: " SOLOIST_API_KEY; export SOLOIST_API_KEY; echo
curl -fsSL https://jedrek0429.github.io/soloist-display/install.sh | sh -s -- --yes
unset SOLOIST_API_KEY
```

For an existing External Soloist, use `soloist-display install --yes --external-soloist --soloist-ws ws://127.0.0.1:9090/`. Add `--start-external` only if you authorise starting its service.

## Uninstall options

Interactive:

```sh
soloist-display uninstall
```

Unattended, with safe defaults:

```sh
soloist-display uninstall --yes --data keep
```

For Managed, choose `--soloist restore`, `--soloist keep` or `--soloist remove`. Use `--data keep|remove` separately.

- `restore` needs a saved previous service. It restores that service before removing the managed binary.
- `remove` is available only when no previous Soloist installation needs restoring.
- `keep` leaves Managed Soloist running, **without automatic build renewal**.
- External Soloist is never an uninstall target.

`--yes` chooses `restore` if a previous service exists, otherwise `keep`. Display settings are kept unless you choose `--data remove`. You can supply both choices explicitly instead of `--yes`.

## Trust

Display release binaries use signed manifests and verified checksums. Periodic Spotify Soloist builds come from Spotify's HTTPS CDN; Spotify does not provide an independently signed checksum here. A hash calculated after download is not a signature. See [installer details](installer.md).
