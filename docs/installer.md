# Installer architecture

`install.sh` detects the Linux userland, downloads a target-specific manager, verifies a signed bootstrap manifest and the binary SHA-256 digest, then runs the manager as the current user.

The manager stores immutable releases below `$XDG_DATA_HOME/soloist-display/releases` and atomically switches the `current` symlink only after verification. Configuration, state, cached downloads, transaction journals, and logs use their respective XDG directories. `~/.local/bin/soloist-display` points to the active manager.

All mutating commands share an advisory filesystem lock. Each install or update writes a durable journal before activation; an interrupted transaction is recovered on the next run. The previous release is retained and restored when activation, service startup, or health checks fail. Package-manager operations are recorded but are not described as atomically reversible.

The manager owns only paths recorded in its installation metadata. Existing Soloist executables, data, credentials, and service definitions remain independently managed. Installer-owned Soloist resources are removed only by `uninstall --mode all`.

The display service binds HTTP to `127.0.0.1:8080` and the audio WebSocket to `127.0.0.1:9091`. Soloist is expected on `127.0.0.1:9090`. Port conflicts fail with diagnostics; the installer never terminates unrelated processes. Audio capture follows the current PulseAudio/PipeWire default sink monitor unless `SOLOIST_DISPLAY_MONITOR` is explicitly set.
