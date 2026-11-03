# Troubleshooting

Start here:

```sh
soloist-display doctor
journalctl --user -u soloist-display.service -u soloist.service -n 100 --no-pager
```

Don't post API keys, tokens or environment files in an issue.

## Command not found

Open a new terminal. Check `~/.local/bin/soloist-display` exists and `~/.local/bin` is in `PATH`.

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

The timer checks every six hours. `renew-soloist` only works for Managed. A failed build should roll back; `doctor` can reveal a broken service. `--force` bypasses the unchanged-build shortcut, not safety checks.

An unreachable CDN leaves the existing build installed. A healthy WebSocket alone does not prove the right binary started; the manager also checks the systemd process.

## Visualiser is flat

Play audio through the computer running Display. Increase sensitivity in settings. Check the default PipeWire/PulseAudio output sink; `SOLOIST_DISPLAY_MONITOR` selects another monitor if needed.

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
