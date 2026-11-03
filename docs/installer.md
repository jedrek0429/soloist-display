# Installer

`install.sh` detects Linux and the CPU, verifies the signed bootstrap manifest and installer checksum, then runs the Rust manager as your user. Installing system dependencies may still require sudo. The standard `curl -fsSL https://jedrek0429.github.io/soloist-display/install.sh | sh` keeps prompts interactive when a controlling terminal exists. For headless installations pass `--yes`.

Display releases live under `$XDG_DATA_HOME/soloist-display/releases`. The active release uses an atomic `current` symlink. Config, cache, logs and transaction journals live in their XDG directories; `~/.local/bin/soloist-display` points to the active manager.

Mutating CLI operations share a lock. Installation, renewal and uninstall use recovery journals. Recovery runs on the next relevant CLI invocation. If the previous build is also broken, manual recovery may be needed. Package-manager changes are not atomically reversible.

## Soloist ownership

- **Managed:** Display owns the Soloist binary and systemd user service. On migration it saves the old service and clones the existing session; it refuses unsupported systemd settings. The original installation is not deleted.
- **External:** Display connects to the existing Soloist WebSocket. It does not replace the service or binary. A stopped External service is started only with consent.

Managed Soloist checks Spotify's CDN every six hours using `soloist-display-renew.timer`. You can run `soloist-display renew-soloist` yourself. Before accepting a build, the manager checks the archive, ELF target and SHA-256. After restart it checks the systemd process, executable and WebSocket listener. Failed activation restores the previous build.

Display release artefacts are verified against signed manifests. **Spotify's rolling builds are different:** HTTPS to the official CDN is the trust boundary. They have no independently verified upstream signature.

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
