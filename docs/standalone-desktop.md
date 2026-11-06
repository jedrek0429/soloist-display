# Desktop (Windows x86_64 and macOS Apple Silicon)

Desktop remains **External Spotify Soloist only**: it installs and maintains Soloist Display, not Spotify Soloist. Intel macOS is not supported. The alpha builds are not OS code-signed, so Windows SmartScreen or macOS Gatekeeper may warn.

## Install

Use the bootstrap from the **specific alpha/beta GitHub Release** that you want to install, so its embedded SHA-256 is pinned to the exact release. Published `install.ps1` and `install.sh` validate the downloaded executable against that hash. The executable subsequently validates the Ed25519-signed release manifest and artifact SHA-256 before installing files.

Windows PowerShell:
```powershell
Invoke-WebRequest "https://github.com/jedrek0429/soloist-display/releases/download/<TAG>/install.ps1" -OutFile install.ps1
.\install.ps1
```

macOS Apple Silicon:
```sh
curl -fL "https://github.com/jedrek0429/soloist-display/releases/download/<TAG>/install.sh" -o install.sh
sh install.sh
```

Replace `<TAG>` with a published version such as `v1.1.0-alpha.2` **only if that release includes the new installer assets**; earlier alphas will not. Download scripts only from the official release. The bootstrap `install.sh` also continues to support Linux unchanged. Stable releases without desktop artifacts cannot install desktop builds.

The installed executable is versioned under `%LOCALAPPDATA%\soloist-display\releases\` (Windows) or `~/Library/Application Support/soloist-display/app/releases/` (macOS). A managed command shim points to the active version: `soloist-display.cmd` in the Windows user bin directory, or `~/.local/bin/soloist-display` on macOS. Windows install.ps1 adds the user bin directory to the user PATH; restart your terminal. On macOS ensure `~/.local/bin` is on PATH.

## Lifecycle commands

```sh
soloist-display doctor
soloist-display check-updates
soloist-display update --channel alpha
soloist-display update --version 1.1.0-alpha.3
soloist-display update --allow-downgrade --version 1.1.0-alpha.2
soloist-display notifications off
soloist-display notifications on
soloist-display repair
soloist-display uninstall --data keep
soloist-display uninstall --data remove
```

For Windows run these via `soloist-display.cmd` (or `soloist-display` after PATH is refreshed). These commands operate **only on Display**. They never install, control, update or remove an External Soloist instance. Updates verify the signed manifest, select the platform-specific binary and run a version smoke check before switching the command shim. If download or verification fails, the old version remains active. No background update timer is installed in this phase. `notifications` controls the nonblocking update check when the installed Display starts.

On Windows, close Display before uninstalling. Locked EXE files cause uninstall to fail without discarding installation metadata. Configuration data is preserved unless you pass `--data remove`. The bootstrap scripts are not automatic elevation mechanisms and do not change system-wide permissions.

## Portable standalone ZIPs

For one-off testing without installing, the original unsigned preview archives remain available:

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

These standalone ZIP previews remain **unsigned**. From the first Phase II release, the separate raw desktop binaries are included in the Ed25519-signed release manifest for installation and updates. PR checks validate code without uploading duplicate branch snapshots; alpha/beta release publication requires both tagged desktop builds and their smoke tests to pass. macOS Gatekeeper and Windows SmartScreen may warn about these binaries. Do not bypass security controls on an untrusted download. A passing CI smoke test verifies that the local pages start; it does not verify real audio capture or Spotify playback on desktop hardware.

Linux installation, systemd services, Managed Soloist, and Linux asset signing remain unchanged.
