# Soloist Display

A minimal fullscreen now-playing display for [Spotify Soloist](https://github.com/spotify/soloist).

Features: circular rotating album art, centred clock, track and artist, live playback progress without timestamps or knobs, album-derived accent colours, smooth track transitions and an optional **real-audio** visualiser. The responsive layout supports 4:3 as well as modern screen ratios.

## Run

1. Start Soloist with its local WebSocket API enabled at `127.0.0.1:9090` (see [official documentation](https://developer.spotify.com/documentation/soloist/reference/websocket-api)).
2. In the project directory run `python -m http.server 8080 --bind 127.0.0.1`.
3. Open `http://127.0.0.1:8080` and press F11.

Edit `config.json` for the Soloist WebSocket endpoint and clock locale. Playback is controlled through Spotify Connect.

## Live audio spectrum on Linux

The bars are silent without audio input, never simulated. Install `numpy`, `websockets`, and `parec` (PipeWire's PulseAudio compatibility works). On Arch Linux:

```sh
sudo pacman -S python python-numpy python-websockets libpulse
pactl list short sources
SOLOIST_DISPLAY_MONITOR='YOUR_OUTPUT.monitor' python tools/audio-bridge.py
```

The bridge listens locally on port 9091. The monitor source should correspond to the device playing the music.

## Security and compatibility

Soloist's WebSocket API has no authentication or browser-origin validation. **Keep both WebSocket ports bound to localhost and never forward them to the internet.** The UI runs as local static files, contains no Spotify credentials, and collects no analytics. Cover colour extraction depends on artwork CORS permissions; if restricted, the previous accent remains.

MIT licensed. Independent project, not affiliated with Spotify.
