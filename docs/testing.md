# Testing

Test source builds without running the release installer.

```sh
git clone https://github.com/jedrek0429/soloist-display.git
cd soloist-display
git switch main
git pull --ff-only

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
sh -n install.sh
```

## Run locally

```sh
mkdir -p /tmp/soloist-test
XDG_CONFIG_HOME=/tmp/soloist-test \
SOLOIST_DISPLAY_HTTP_PORT=8088 \
SOLOIST_DISPLAY_AUDIO_PORT=9191 \
./target/release/soloist-audio-bridge
```

Open http://127.0.0.1:8088 (settings: `/settings`).

Uses separate configuration and ports; leaves the installed service alone. Soloist defaults to port `9090`. For the visualizer, set `audioWebSocket` to `ws://127.0.0.1:9191` in test settings.

Check playback, display, settings, transitions, audio, and reconnection.

Do not run `install`, `update`, or `uninstall` against your regular setup. Test installer and service changes in a disposable environment.
