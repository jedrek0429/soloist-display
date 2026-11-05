# Standalone desktop preview (External Soloist only)

These Windows and macOS builds are **experimental** and do not install anything. They do not contain the Linux installer, start services, manage Spotify Soloist, update binaries or provide a Managed mode.

Download the matching `soloist-display` executable from the **Standalone desktop (External Soloist)** GitHub Actions artifact, then run it from a terminal:

- Windows x86_64: `./soloist-display.exe` (PowerShell)
- macOS Apple Silicon or Intel: `./soloist-display`

Open **http://127.0.0.1:8080**. Settings are at **http://127.0.0.1:8080/settings**.

Start your existing Spotify Soloist separately. Display connects to its WebSocket at `ws://127.0.0.1:9090` by default. If you use another local endpoint, change **Soloist** in Display settings. While Soloist is offline, the UI shows its waiting/reconnecting state; it must never install or start Soloist itself.

The spectrum uses CPAL to capture the computer's **default audio output**, not a microphone. macOS output loopback requires macOS **14.6 or later**. The current preview still needs hardware testing on both operating systems.

Settings are saved in the user's native configuration directory: `%APPDATA%\\soloist-display\\config.json` on Windows, `~/Library/Application Support/soloist-display/config.json` on macOS. Set `SOLOIST_DISPLAY_CONFIG_DIR` to override the settings directory for isolated tests. The HTTP and spectrum ports can be overridden using `SOLOIST_DISPLAY_HTTP_PORT` and `SOLOIST_DISPLAY_AUDIO_PORT`.

These are unsigned CI test executables, **not published releases or installers**. macOS Gatekeeper and Windows SmartScreen may warn about them. Do not bypass security controls on an untrusted download. A passing CI smoke test verifies that the local pages start; it does not verify real audio capture or Spotify playback on desktop hardware.

Linux installation, systemd services, Managed Soloist and the signed release pipeline remain unchanged.
