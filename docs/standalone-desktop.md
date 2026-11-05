# Standalone desktop preview (External Soloist only)

These Windows x86_64 and macOS Apple Silicon builds are **experimental** and do not install anything. Spotify Soloist does not support Intel Macs, so macOS x86_64 binaries are not built. They do not contain the Linux installer, start services, manage Spotify Soloist, update binaries or provide a Managed mode.

**Versioned alpha/beta preview:** open [GitHub Releases](https://github.com/jedrek0429/soloist-display/releases), choose the desired `vX.Y.Z-alpha.N` or `vX.Y.Z-beta.N` release, and download `soloist-display-preview-windows-x86_64-<tag>.zip` or `soloist-display-preview-macos-arm64-<tag>.zip` under **Assets**. These are built from the exact tagged commit, and each ZIP contains `BUILD.txt` with its tag and commit SHA. Preview ZIPs are not produced for stable releases.

**Integrated branch snapshots:** [Desktop prerelease builds](https://github.com/jedrek0429/soloist-display/actions/workflows/standalone-desktop.yml) still publishes branch-specific artifacts on successful `alpha/*` and `beta/*` pushes. Unlike release downloads, these are temporary GitHub Actions artifacts (14-day retention, potentially requiring GitHub sign-in) and contain the branch commit, not necessarily the tagged release commit.

Unzip the archive and run from a terminal:

- Windows x86_64: `./soloist-display.exe` (PowerShell)
- macOS Apple Silicon (ARM64): `chmod +x ./soloist-display && ./soloist-display`

Open **http://127.0.0.1:8080**. Settings are at **http://127.0.0.1:8080/settings**. Run `soloist-display.exe doctor` on Windows or `./soloist-display doctor` on macOS to check the External Soloist protocol and visualiser output device without managing processes.

**Audio visualiser output selection** (use the corresponding executable on macOS):

```powershell
.\soloist-display.exe device list
.\soloist-display.exe device use 2
.\soloist-display.exe device default
```

Use the number printed by `device list`, a stable device ID or an exact, unambiguous device name. The list number is only for convenience; the saved selection always uses the stable ID. `device set <id>` remains an alias for `device use`. The choice is saved in `audio-device.id` beside the Display settings and takes effect automatically in a running audio bridge. `device default` removes the override and follows the system's default output. These commands select **only the system audio output captured by the audio bridge for the visualiser**, not the Spotify Connect playback device; they never change the OS default or install/manage Soloist. A selected output that disappears produces an explicit capture error instead of silently switching to the wrong device. Linux continues to support the legacy `SOLOIST_DISPLAY_MONITOR` override when no device was explicitly selected.

Start your existing Spotify Soloist separately. Display connects to its WebSocket at `ws://127.0.0.1:9090` by default. If you use another local endpoint, change **Soloist** in Display settings. While Soloist is offline, the UI shows its waiting/reconnecting state; it must never install or start Soloist itself.

The spectrum uses CPAL to capture the computer's **default audio output**, not a microphone. macOS output loopback requires macOS **14.6 or later**. The current preview still needs hardware testing on both operating systems.

Settings are saved in the user's native configuration directory: `%APPDATA%\\soloist-display\\config.json` on Windows, `~/Library/Application Support/soloist-display/config.json` on macOS. Set `SOLOIST_DISPLAY_CONFIG_DIR` to override the settings directory for isolated tests. The HTTP and spectrum ports can be overridden using `SOLOIST_DISPLAY_HTTP_PORT` and `SOLOIST_DISPLAY_AUDIO_PORT`.

These desktop executables are **experimental and unsigned**, including when attached to GitHub Releases. They are not installers and are excluded from the signed Linux manifests. PR checks validate code without uploading duplicate branch snapshots; alpha/beta release publication requires both tagged desktop builds and their smoke tests to pass. macOS Gatekeeper and Windows SmartScreen may warn about these binaries. Do not bypass security controls on an untrusted download. A passing CI smoke test verifies that the local pages start; it does not verify real audio capture or Spotify playback on desktop hardware.

Linux installation, systemd services, Managed Soloist, and Linux asset signing remain unchanged.
