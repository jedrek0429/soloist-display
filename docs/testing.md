# Testing

Build from source. Leave the published installer out of it.

```sh
git clone https://github.com/jedrek0429/soloist-display.git
cd soloist-display

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo build --workspace --release --locked
sh -n install.sh
```

## Run the display locally

```sh
mkdir -p /tmp/soloist-test
XDG_CONFIG_HOME=/tmp/soloist-test \
SOLOIST_DISPLAY_HTTP_PORT=8088 \
SOLOIST_DISPLAY_AUDIO_PORT=9191 \
./target/release/soloist-audio-bridge
```

Open http://127.0.0.1:8088. Point `audioWebSocket` at `ws://127.0.0.1:9191` in test settings. Soloist still defaults to port `9090`.

## Preview idle appearances

Open `docs/idle-preview.html` in a browser directly. No server or Spotify session is needed. The page uses the display's real standby markup and stylesheet, with controls for all five appearances, a custom message, and connection status. Press **P** to hide or show the preview controls.

## Standby fade

In **Motion → Idle**, select **Transition → Fade**, then test **Duration** at 0, 300 and 2000 ms. Check entry and exit, including **After pausing → 0** and **Pure black**. Set **Transition → Off** and verify both directions are instant and Duration is hidden. Reduced motion must still skip the transition.

## Test the installer

Use a disposable Linux user or VM. The live systemd tests can create, start and remove `soloist.service`. **Do not run them on your usual setup.**


Before a release, also test with a real Spotify Connect session:

- Fresh Managed and External installs; External → Managed migration.
- Existing custom service: either preserve its settings and session or refuse before takeover.
- Successful renewal, broken new build and interrupted renewal.
- `repair`, `doctor` and interrupted uninstall.
- Uninstall with `restore`: the old service plays again.

No green tick can replace a real speaker.

## Pause sleeve retraction

With Vinyl & Cover or CD & Cover selected, set **Sleeve motion → Retract after pause** to a time or Never. This is separate from **Standby → After pausing**.

Verify that a paused record slides behind its stationary cover, resuming slides it out, and a track change or disconnect during the slide does not leave the wrong record exposed. Other disc appearances must remain unaffected.
