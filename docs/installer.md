# Installer

`install.sh` detects Linux and the CPU, verifies the signed bootstrap manifest and installer checksum, then runs the Rust manager as your user. Installing system dependencies may still require sudo. The standard `curl -fsSL https://jedrek0429.github.io/soloist-display/install.sh | sh` keeps prompts interactive when a controlling terminal exists. For headless installations pass `--yes`.

Display releases live under `$XDG_DATA_HOME/soloist-display/releases`. The active release uses an atomic `current` symlink. Config, cache, logs and transaction journals live in their XDG directories; `~/.local/bin/soloist-display` points to the active manager.

Mutating CLI operations share a lock. Installation, renewal and uninstall use recovery journals. Recovery runs on the next relevant CLI invocation. If the previous build is also broken, manual recovery may be needed. Package-manager changes are not atomically reversible.

## MPRIS bridge lifecycle

The opt-in add-on downloads only the [soloist-mpris](https://github.com/TobiasVeiga00/soloist-mpris) binary (currently pinned to v0.2.2); it never runs upstream's installer, replaces `soloist.service`, changes credentials or creates upstream update timers. The manager checks the downloaded archive against upstream's SHA-256 file, validates its ELF architecture and stores the upstream MIT licence alongside the executable. The checksum is published beside the archive: it detects corruption, **not** author authenticity.

Owned files:

- `$XDG_DATA_HOME/soloist-display/addons/mpris/soloist-mpris` and `LICENSE`
- `$XDG_CONFIG_HOME/systemd/user/soloist-display-mpris.service`
- `$XDG_STATE_HOME/soloist-display/mpris.json`

The separate `soloist-display-mpris.service` publishes `org.mpris.MediaPlayer2.soloist` on the session D-Bus. `SOLOIST_DATA_DIR` points to Soloist's data directory; the bridge reads `ws.addr` and `ws.port` there and reconnects after restarts. Managed uses its configured data directory; External can specify one. The add-on is supported on x86_64 and ARM64, not ARMv7.

Installation refuses to overwrite existing services, unmanaged add-on files or another owner of the MPRIS name. Interrupted installation is recovered on the next manager operation. Soloist Display uninstall stages the owned files in its recovery journal, snapshots the bridge service state and restores it on rollback. External Soloist and independently installed bridges are not uninstall targets. See [advanced installation](advanced-installation.md#optional-mpris-bridge) for user commands.

## Soloist ownership

- **Managed:** Display owns the Soloist binary and systemd user service. On migration it saves the old service and clones the existing session; it refuses unsupported systemd settings. Legacy `EnvironmentFile=` accepts absolute paths and `%h/` home-directory paths. The original installation is not deleted.
- **External:** Display connects to the existing Soloist WebSocket. It does not replace the service or binary. A stopped External service is started only with consent.

## Managed Soloist locations

With the default XDG directories, the Managed Soloist binary is installed at:

```text
~/.local/share/soloist-display/soloist/current/soloist
```

Its systemd user service is:

```text
~/.config/systemd/user/soloist.service
```

These paths use `$XDG_DATA_HOME` and `$XDG_CONFIG_HOME` if set. `current` is a symlink that follows Soloist renewals.

The installer does not put the Managed Soloist CLI on your `PATH`. To run `soloist` from a terminal, add its containing directory:

```sh
export PATH="${XDG_DATA_HOME:-$HOME/.local/share}/soloist-display/soloist/current:$PATH"
```

Put that line in `~/.bashrc` or `~/.zshrc` to make it persistent. The systemd service does not need this. External Soloist uses its existing installation paths.

Managed Soloist checks Spotify's CDN every six hours using `soloist-display-renew.timer`. You can run `soloist-display renew-soloist` yourself. Before accepting a build, the manager checks the archive, ELF target and SHA-256. After restart it checks the systemd process, executable and WebSocket listener. Failed activation restores the previous build.

Display release artefacts are verified against signed manifests. **Spotify's rolling builds are different:** initial Managed installation and automatic renewal both download the current build directly from Spotify's HTTPS CDN. The installer limits the download size and verifies the archive and executable architecture; the build is not pinned to an old Display release manifest. Spotify does not provide an independently verified upstream signature. When prompted, obtain a Soloist API key from [Spotify for Developers](https://developer.spotify.com/dashboard/soloist).

## Uninstall

`soloist-display uninstall` asks two questions: what to do with Soloist, and whether to keep Display settings.

For Managed, choose `--soloist restore|keep|remove`. `restore` requires a previous service; `remove` is refused if one needs restoring. `keep` retains Soloist but stops automatic renewal. `--data keep|remove` controls Display settings and cache. External Soloist is untouched.

Uninstall stages owned files before deleting them. A failed operation can recover them from its journal. A completed removal is not rolled back.

## Services

- Display HTTP: `127.0.0.1:8080`
- Audio WebSocket: `127.0.0.1:9091`
- Soloist WebSocket: `127.0.0.1:9090` by default; External may use another loopback port
- Audio: default PipeWire/PulseAudio sink monitor, or `SOLOIST_DISPLAY_MONITOR`

Port conflicts stop installation. Unrelated processes are not killed.

## Releases

Bootstrap supports `SOLOIST_DISPLAY_VERSION` and `SOLOIST_DISPLAY_CHANNEL=stable|prerelease`. Version selection and downgrades are covered in [advanced installation](advanced-installation.md); signing and publishing are covered in [releasing](releasing.md).
