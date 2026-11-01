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

## Run

Start Soloist with its local WebSocket on `127.0.0.1:9090`.

```sh
python -m http.server 8080 --bind 127.0.0.1
```

Open `http://127.0.0.1:8080` and use fullscreen (F11).

For the real audio visualiser on Arch Linux:

```sh
sudo pacman -S rust libpulse
pactl list short sources
SOLOIST_DISPLAY_MONITOR='YOUR_OUTPUT.monitor' cargo run --release --manifest-path audio-bridge/Cargo.toml
```

Replace `YOUR_OUTPUT.monitor` with the actual output monitor for the Spotify audio stream. If it is wrong or silent, the visualiser will not work. The Rust bridge reports actual FFT frame rate, signal peak, six frequency-band levels and connected clients every five seconds for troubleshooting.

`config.json` configures the Soloist and spectrum WebSocket URLs. Restart the Rust bridge after updating: the old binary PCM protocol is incompatible with the new JSON six-band spectrum protocol. No browser audio permission or Web Audio activation is required.

## Validation status

This branch has not yet been compiled or tested on the physical CRT. Verify the Rust build, WebSocket message cadence, artist metadata and actual disc transitions before merging PR #1. Both WebSocket endpoints are localhost-only and must not be exposed publicly.
