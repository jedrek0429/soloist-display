# Known issues
## Windows audio output switching

Currently, Soloist on Windows does not automatically follow changes to the system's default audio output while the original device remains available. [This has been reported](https://github.com/spotify/soloist/issues/30).

Soloist Display's audio visualiser should switch to the new default device independently, causing the visualiser to stop responding to playback.

Until the Soloist issue is fixed (which is unlikely any time soon), **restart Soloist to make it use the new default audio output**. 

# Troubleshooting

Start here:

```sh
soloist-display doctor
journalctl --user -u soloist-display.service -u soloist.service -n 100 --no-pager
```

Don't post API keys, tokens or environment files in an issue.

## Command not found

For `soloist-display`, open a new terminal. Check `~/.local/bin/soloist-display` exists and `~/.local/bin` is in `PATH`.

For `soloist`, Managed installations keep the executable outside `PATH`. See [Managed Soloist locations](installer.md#managed-soloist-locations) for the binary, systemd unit and shell setup.

## Display won't open

```sh
systemctl --user status soloist-display.service
journalctl --user -u soloist-display.service -n 100 --no-pager
```

Default address: http://127.0.0.1:8080. If the port is taken, find the culprit. Don't kill random processes.

## Spotify Connect is missing

Spotify Premium and a valid Soloist API key are required.

```sh
systemctl --user status soloist.service
journalctl --user -u soloist.service -n 100 --no-pager
```

Managed Soloist is started by Display. External Soloist remains your responsibility; the installer does not silently take it over. Check the configured loopback WebSocket port (normally `9090`).

## Managed Soloist won't renew

```sh
systemctl --user status soloist-display-renew.timer
journalctl --user -u soloist-display-renew.service -n 100 --no-pager
soloist-display renew-soloist
```

The timer checks weekly. `renew-soloist` only works for Managed. A failed build should roll back; `doctor` can reveal a broken service. `--force` bypasses the unchanged-build shortcut, not safety checks.

An unreachable CDN leaves the existing build installed. A healthy WebSocket alone does not prove the right binary started; the manager also checks the systemd process.

## Visualiser is flat

Play audio through the computer running Display. Increase sensitivity in settings. Check the default PipeWire/PulseAudio output sink; `SOLOIST_DISPLAY_MONITOR` selects another monitor if needed. CPAL records the output monitor, not a microphone. Use `pactl list short sources` to check that a `.monitor` source exists. If `libasound.so.2` is missing, install `alsa-lib` (Arch/Fedora) or `libasound2t64` (Ubuntu 24.04/Debian 13), then restart the Display service.

## Install, update or uninstall failed

Read the error and its reported log path. Then try:

```sh
soloist-display doctor
soloist-display repair
```

The next mutating CLI command attempts recovery of interrupted transactions. If recovery itself fails, **keep the state files and journal**.

Signed-release verification failures must not be bypassed. Managed Spotify builds use HTTPS to Spotify's official CDN rather than a signed upstream manifest.

## Report a bug

[Open a bug report](https://github.com/jedrek0429/soloist-display/issues/new?template=bug_report.yml) with your distribution, architecture, version, the exact error, `doctor` output and sanitised logs.

See [advanced installation](advanced-installation.md) for install flags and [installer](installer.md) for what the manager actually touches.
