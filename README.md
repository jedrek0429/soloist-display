# Soloist Display

An artwork-first, fullscreen now-playing display for Spotify Soloist. Works on 4:3 and widescreen displays.

The interface is deliberately minimal: an oversized rotating circular album cover, a small local clock, track title and artist, a genuine audio spectrum and an oversized progress bar without timestamps or a scrubber. Background and accent colours are derived from album artwork, with neutral fallback colours and no glow.

## Motion design

Track changes use a staged broadcast-style disc exchange. The old disc exits, the next enters as a large centred hero image, holds for a second, then moves into the oversized left-hand now-playing position. The clock and metadata reveal from under the disc. New track changes interrupt the current sequence; no transition queue. Rotation is driven by a continuous animation loop, with acceleration and deceleration on play/pause.

## Run locally

1. Run Soloist with its WebSocket API listening on localhost, normally port 9090.
2. Run `python -m http.server 8080 --bind 127.0.0.1` in this repository (Python is only an optional static-file server; the audio backend is Rust).
3. Open `http://127.0.0.1:8080` and press F11.
4. Control music with Spotify Connect on your phone.

Change WebSocket URLs or clock locale in `config.json`. If the album cover host does not permit cross-origin pixel reads, the UI retains its last palette.

## Rust audio bridge (Linux / PipeWire)

Install `rust`, `cargo` and `libpulse` (for `parec`) using your distribution's package manager. PipeWire's PulseAudio compatibility layer supports the same monitor source.

```sh
# Arch Linux
sudo pacman -S rust libpulse
pactl list short sources
SOLOIST_DISPLAY_MONITOR='YOUR_OUTPUT.monitor' cargo run --release --manifest-path audio-bridge/Cargo.toml
```

Choose the monitor source associated with the output playing Spotify. The bridge captures real PCM, calculates a 48-band FFT spectrum at approximately 30 Hz and publishes it over a localhost WebSocket on port 9091. The browser smooths values at its own frame rate. Without the bridge, the bars remain still rather than generating fake visualiser activity.

## Security

Both WebSocket endpoints are intended for **localhost only**. Do not expose the Soloist control WebSocket to the network or internet. This is a static local UI with no analytics or credentials. The bridge binds to 127.0.0.1.

## Status

This is an initial integration build. The exact Soloist event structure and artwork URL shapes should be checked against a real running Soloist instance. Report any metadata mismatches with a redacted example event. The interface is not yet validated on physical CRT hardware.

MIT licensed. Not affiliated with Spotify.
