# Advanced installation

The [README](../README.md) covers normal use.

## Linux audio requirements

The GNU/Linux releases require **glibc 2.38+** and `libasound.so.2`. The installer installs ALSA's runtime library when missing (`alsa-lib` on Arch/Fedora, `libasound2t64` on Ubuntu 24.04 and Debian 13). No ALSA configuration or microphone permission is required for capture.

CPAL records the **output monitor** exposed by PulseAudio or `pipewire-pulse`. It does not record a microphone or require `parec`. The default output monitor is selected automatically; `SOLOIST_DISPLAY_MONITOR` can select another source. The `pactl` command is still used to diagnose the output monitor.

The three release targets are `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` and `armv7-unknown-linux-gnueabihf` (hard-float). They use the host's ALSA runtime instead of shipping statically linked `musl` executables.

## Pick a release

The bootstrap selects the latest stable release, or the latest prerelease if there is no stable release yet.

```sh
curl -fsSL https://jedrek0429.github.io/soloist-display/install.sh | SOLOIST_DISPLAY_VERSION=1.0.0-alpha.1 sh
```

After installation:

```sh
soloist-display check-updates
soloist-display update --channel stable
soloist-display update --channel alpha
soloist-display update --channel beta
soloist-display update --channel prerelease
soloist-display update --version 1.1.0-beta.67
soloist-display update --version 1.0.0-alpha.1 --allow-downgrade
soloist-display notifications off
```

The channel is remembered. `alpha` and `beta` track only their respective prereleases. The legacy `prerelease` channel follows both. A normal install follows stable once it exists; setting `SOLOIST_DISPLAY_CHANNEL=alpha` or `beta` selects that channel when the manager resolves a release. Downgrades need explicit consent; older settings may not survive them. `notifications on` restores update prompts.

## Shell completions

The installer sets up completions for Bash, Zsh and Fish. Bash needs the `bash-completion` package enabled. Zsh is configured through an installer-owned `~/.zshrc` block; Bash and Fish discover their completion files without editing their startup files.

After installing a new shell, `soloist-display update` refreshes completions even if Display is already up to date. You can also generate scripts manually with `soloist-display completions bash|zsh|fish`.

## Portable CLI (Windows and macOS previews)

The Windows x86_64 and macOS Apple Silicon releases currently run in **External-only** mode.
They share the CLI flag and audio-device argument definitions with the Linux manager,
but do not yet include the Linux installer or managed service lifecycle.

```sh
soloist-display --help
soloist-display doctor
soloist-display launch       # open the now-playing display
soloist-display settings     # open the settings page
soloist-display device list
soloist-display device use 2 # choose output for the visualiser, not Spotify
soloist-display device default
soloist-display completions powershell # use bash/zsh/fish on other shells
```

Run the executable with no arguments to serve the local display as before.
`launch` and `settings` open the relevant URL in the operating system's
default browser, reusing a running local server when available. If the
standalone server is not running, they start it in the current process,
which must remain running to keep the display available.

Global flags `--no-color`, `--quiet`, `--verbose`, and `--yes`
use the shared parser. Installation and renewal commands still work only
on Linux; `--yes` is reserved for the upcoming desktop installer lifecycle.
Shell completion scripts are generated to stdout and are not automatically installed.

On Linux, `soloist-display device list/use/set/default` delegates to the
installed audio bridge. The device selector affects only the visualiser
capture output and never changes Soloist's playback device.

## Managed or External?

**Managed** is the default. Display installs and maintains a Soloist binary and user service. It checks the official Spotify CDN weekly. To check now:

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
