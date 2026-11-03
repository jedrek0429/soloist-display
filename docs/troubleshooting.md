# Troubleshooting

If Soloist Display does not work as expected, start with:

```sh
soloist-display doctor
```

This checks the installation, services, ports, and audio setup. For more detail, inspect the user-service logs:

```sh
journalctl --user -u soloist-display.service -u soloist.service -n 100 --no-pager
```

## The command is not found

Open a new terminal after installation. If it is still unavailable, check whether the executable exists:

```sh
ls -l ~/.local/bin/soloist-display
```

If the file exists, make sure `~/.local/bin` is included in your shell's `PATH`.

## The display will not open

Run `soloist-display doctor`. Check the display service and its logs:

```sh
systemctl --user status soloist-display.service
journalctl --user -u soloist-display.service -n 100 --no-pager
```

The display normally serves `http://127.0.0.1:8080`. If the address does not respond, inspect the service errors. Do not kill unrelated processes occupying the port.

## Spotify Connect device is missing or playback does not work

Spotify Premium is required. Ensure that Spotify Soloist is running and configured with a valid API key. If Soloist was installed separately, the installer preserves it rather than taking ownership of its service.

```sh
systemctl --user status soloist.service
journalctl --user -u soloist.service -n 100 --no-pager
```

Soloist is expected to serve its local WebSocket at `127.0.0.1:9090`. When troubleshooting, avoid posting API keys or environment files in public issues.

## Audio visualizer is not moving

Ensure that music is playing through the computer running Soloist Display. The visualizer captures audio from the PulseAudio/PipeWire default sink monitor; playback routed to another output may not appear.

You may need to increate sensitivity in Soloist Display settings.

Check `soloist-display doctor` and verify the default audio sink. Advanced users can choose another monitor with `SOLOIST_DISPLAY_MONITOR`.

## Installation or update failed

Read the error message and check the installer log:

```sh
soloist-display doctor
```

A failed signed-manifest or checksum verification must not be bypassed. Retry once after checking network connectivity; if it persists, report the failing release and exact error. Do not disable signature verification.

For incomplete or damaged installer-managed files, use:

```sh
soloist-display repair
```

The installer is designed to retain the previous release and restore it if activation or health checks fail.

## Getting more help

When [opening a GitHub issue](https://github.com/jedrek0429/soloist-display/issues), include your Linux distribution and architecture, the Soloist Display version, the exact error, relevant `doctor` output, and sanitized logs. Remove API keys, tokens, and personal details.

See [advanced installation](advanced-installation.md) for release channels, downgrades, and unattended setup, or [installer architecture](installer.md) for implementation details.
