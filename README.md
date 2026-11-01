# Soloist Display

Minimal fullscreen now-playing display for Spotify Soloist, designed for 4:3 CRT televisions.

## Visual design

- One enormous, **centred, rotating album disc**, intentionally cropped at the top and bottom.
- Full-bleed **album artwork background** with strong Gaussian blur and a uniform semi-transparent black overlay. No generated grey/green palette, glow, fake grooves, or CRT filters.
- Small, unboxed lower-right metadata: song title, artist, real-audio miniature spectrum, and thin progress line. No clock.
- Track changes use two separate disc elements: outgoing disc exits, incoming disc enters oversized, pauses for a one-second hero showcase, then settles. Text reveals only after the disc arrives.
- On initial load, the current track appears immediately instead of playing an unnecessary entrance sequence.

## Architecture

- `src/soloist.js`: Soloist event and nested artist/album parsing.
- `src/motion.js`: Disc exchange choreography and continuous rotation.
- `src/spectrum.js`: six-band Canvas rendering and interpolation; no browser audio analysis.
- `src/main.js`: WebSocket connections, playback state and progress.
- `audio-bridge/`: Rust PipeWire/PulseAudio monitor capture, overlapping 2048-sample FFT windows every 512 samples (~43 Hz), per-band smoothing and JSON WebSocket output.

## Install on Arch Linux

Spotify Soloist must already be running and expose its local WebSocket on port 9090.

From a checkout of the **feat/now-playing-display** branch:

```sh
makepkg -si
```

Open **Soloist Display** or **Soloist Display Settings** from Rofi. Both desktop
entries are installed by the package. The launcher starts the background systemd
user service, waits until the local HTTP server is ready, then opens Firefox.
The main display uses Firefox kiosk mode. Closing Firefox does not stop the
background audio/server service; it can be stopped with:

```sh
systemctl --user stop soloist-display
```

Logs: `journalctl --user -u soloist-display -f`.

The one Rust binary starts the audio spectrum WebSocket on
`127.0.0.1:9091`, and serves the display and settings at
`http://127.0.0.1:8080/` and `http://127.0.0.1:8080/settings`.
The separate Python HTTP server is no longer required. The WebSocket
connection to Spotify Soloist on port 9090 remains a separate service.

Settings are edited in the browser, persisted at
`~/.config/soloist-display/config.json`, and applied to the display
automatically (via a page refresh when settings change). A reset-to-defaults
button is available. The built-in defaults are bundled with the binary; manual
JSON editing is optional.

The audio bridge discovers the default PipeWire/PulseAudio sink monitor.
Override it with `SOLOIST_DISPLAY_MONITOR` in a systemd user service override
if necessary. Both HTTP and spectrum services bind to localhost only.

For development without installing:

```sh
cargo run --release --manifest-path audio-bridge/Cargo.toml
```

Open `http://127.0.0.1:8080/` and `http://127.0.0.1:8080/settings`.
The Rust process serves the bundled frontend; the app does not require
`python -m http.server`.

## Validation status

This branch has not yet been compiled or tested on the physical CRT. Verify the Rust build, WebSocket message cadence, artist metadata and actual disc transitions before merging PR #1. Both WebSocket endpoints are localhost-only and must not be exposed publicly.
