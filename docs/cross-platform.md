# Cross-platform Soloist companion architecture

Soloist Display is a companion and lifecycle wrapper for Spotify Soloist, not a replacement Spotify playback engine. The display may connect to independently installed Soloist; ownership of an existing engine must never be assumed.

## Goals

- Linux, Windows and macOS support without Electron.
- Preserve the existing six-band FFT, WebSocket protocol, visual design and settings.
- Share signed-release verification, update transactions, configuration migrations, rollback and CLI across platforms.
- Allow two explicit Soloist modes: `managed` (installed and supervised by the wrapper) and `external` (detected or configured, never modified/uninstalled).
- Keep platform capabilities honest. An OS is not considered fully supported until compatible Soloist binaries, audio loopback, launch integration, and installation have been tested.

## Migration sequence

1. Extract audio capture behind a Rust interface yielding timestamped interleaved or mono floating-point samples with an explicit sample rate and format. Separate resampling and FFT from backend selection. Linux `parec` monitoring remains a functional fallback until the replacement is validated.
2. Investigate CPAL for device enumeration and host streams. Use platform-specific system-output loopback where CPAL does not expose it reliably. Do not substitute microphone capture for output audio without explicit user selection.
3. Extract shared installer operations from Linux-specific code (`std::os::unix`, systemd, XDG, package managers, process invocation, filesystem links). Add platform adapters for Linux (systemd/XDG), Windows (per-user startup, AppData, WASAPI), and macOS (launchd/Library, CoreAudio). Do not start privileged system services by default.
4. Probe Soloist compatibility and availability per OS and architecture. If no supported engine is available, offer a clearly labelled companion-only mode only when the external engine is actually reachable. Never claim to support a platform because the display alone compiles.
5. Extend CI with Windows and macOS compile/test jobs and OS-specific installation and rollback smoke tests. Keep Linux x86_64, ARM64 and ARMv7 builds. Add signed OS-specific distribution artifacts only after validated support.

## Contracts and regression tests

- An already running Soloist at a configured local endpoint is reused without replacing credentials or service definitions.
- Uninstall of Soloist Display cannot remove external Soloist.
- Managed Soloist updates respect signed release metadata and preserve credentials.
- Audio backend disconnects and sample-format changes recover; silence remains silence; the UI receives six stable bands at the existing cadence.
- Locking, transactional updates, crash recovery and rollback work with Windows and macOS filesystem semantics, not just Unix.
- Existing Linux installs, `soloist-display doctor`, repair, update and uninstall remain functional.
- All OS-specific capabilities have explicit tests and documented limitations.

## Release gate

Do not publish Windows or macOS installers until end-to-end playback through Spotify Soloist, output capture, clean installation, update, repair and uninstall are tested on the respective operating system. Keep the Linux release path unchanged during the migration.
