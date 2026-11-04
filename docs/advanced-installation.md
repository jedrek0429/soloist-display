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

For a supported existing user service, Managed saves its unit and clones its session. The original stays put. Unsupported systemd settings, drop-ins and startup flags stop migration.

## Optional MPRIS bridge

[soloist-mpris](https://github.com/TobiasVeiga00/soloist-mpris) is an **optional** MIT-licensed bridge between Soloist's WebSocket and the Linux session D-Bus (`org.mpris.MediaPlayer2.soloist`). It supports `playerctl`, media keys bound by your desktop or window manager, and MPRIS-compatible bars. Soloist Display does not need the bridge to play or display music.

```sh
soloist-display mpris enable
soloist-display mpris status
playerctl -p soloist play-pause
playerctl -p soloist next
soloist-display mpris disable      # keep files, stop the bridge
soloist-display mpris enable       # start it again
soloist-display mpris remove       # uninstall just the bridge
```

For External Soloist using a non-default data directory, specify the directory containing Soloist's `ws.addr` and `ws.port` files:

```sh
soloist-display mpris enable --data-dir /absolute/path/to/soloist-data
```

Alternatively, you can install Soloist and its MPRIS bridge with the [soloist-mpris installer](https://github.com/TobiasVeiga00/soloist-mpris) by TobiasVeiga00, then connect Soloist Display in **External** mode. The upstream installer already runs `soloist-mpris.service`, so **do not enable** `soloist-display mpris` as well. Upstream Soloist uses a dynamically assigned WebSocket port (`--ws 127.0.0.1:0`); pass its current address from `ws.addr` and `ws.port` using `--soloist-ws`, not the default port `9090`. Soloist Display currently stores that address, so a restart that changes the port requires updating the External connection.

Managed Soloist uses its configured data directory automatically. External Soloist defaults to `$XDG_DATA_HOME/soloist` (usually `~/.local/share/soloist`). Start Soloist first so the WebSocket address files exist. Only x86_64 and ARM64 upstream binaries are available; ARMv7 is not currently supported by this add-on. A working systemd user session and D-Bus are required.

The bridge runs separately from Soloist and reconnects after Soloist restarts. Enabling it never installs another Soloist instance, and an existing MPRIS bridge is not replaced. Removing Soloist Display also removes its own bridge without changing External Soloist.

If the bridge is enabled but does not appear in `playerctl -l`, check `journalctl --user -u soloist-display-mpris.service -n 100 --no-pager` and `soloist-display mpris status`. In a bare window manager, map `XF86AudioPlay` to `playerctl -p soloist play-pause` and similar keys to `next` and `previous`. Installation paths, verification and lifecycle details are documented in [installer internals](installer.md#mpris-bridge-lifecycle).

## Unattended setup

Use `--yes` without a terminal. For Managed, supply the Soloist API key through the environment:

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
