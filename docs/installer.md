# Installer architecture

`install.sh` detects the Linux userland, downloads a target-specific manager, verifies a signed bootstrap manifest and the binary SHA-256 digest, then runs the manager as the current user.

The manager stores immutable releases below `$XDG_DATA_HOME/soloist-display/releases` and atomically switches the `current` symlink only after verification. Configuration, state, cached downloads, transaction journals, and logs use their respective XDG directories. `~/.local/bin/soloist-display` points to the active manager.

All mutating commands share an advisory filesystem lock. Each install or update writes a durable journal before activation; an interrupted transaction is recovered on the next run. The previous release is retained and restored when activation, service startup, or health checks fail. Package-manager operations are recorded but are not described as atomically reversible.

The manager owns only paths recorded in its installation metadata. Existing Soloist executables, data, credentials, and service definitions remain independently managed. Installer-owned Soloist resources are removed only by `uninstall --mode all`.

The display service binds HTTP to `127.0.0.1:8080` and the audio WebSocket to `127.0.0.1:9091`. Soloist is expected on `127.0.0.1:9090`. Port conflicts fail with diagnostics; the installer never terminates unrelated processes. Audio capture follows the current PulseAudio/PipeWire default sink monitor unless `SOLOIST_DISPLAY_MONITOR` is explicitly set.

## Release selection and channels

The bootstrap supports explicit canonical prerelease versions (`SOLOIST_DISPLAY_VERSION=1.0.0-alpha.1`) and verifies signed manifest data for the requested tag. A release-rendered bootstrap pins its manager binary, SHA-256 and matching release version. The default stable manager uses GitHub's `/releases/latest`; an installed prerelease manager discovers the highest published SemVer release (including prereleases). `SOLOIST_DISPLAY_CHANNEL=stable|prerelease` overrides the inferred channel. Explicit `SOLOIST_DISPLAY_VERSION` takes priority. Downgrades require `SOLOIST_DISPLAY_ALLOW_DOWNGRADE=1` and should be treated as potentially unsafe for migrated configuration.

The manager validates the release manifest's Ed25519 signature and checks artifact hashes and sizes. Fallback URLs are not allowed to redirect verification toward a mutable release with a different checksum. The signed version is part of the installation state; interrupted updates retain the previous executable and revert the symlink when activation fails.
