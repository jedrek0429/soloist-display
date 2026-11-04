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

## Optional MPRIS add-on

The installer unit tests cover MPRIS service quoting, checksum validation, ELF architecture rejection, and unsupported ARMv7. `Installer integration` also exercises the generated unit under a live disposable systemd user manager. It runs a stand-in process, not Spotify Soloist.

Before publishing, verify manually with a real Soloist session:

- Managed and External (including a custom data directory).
- Enable, disable, re-enable, and remove, checking `playerctl -p soloist metadata` and media keys.
- An existing externally managed `soloist-mpris.service` or owned D-Bus name must be left untouched.
- Stop/restart Soloist while the bridge is running; check reconnect.
- Uninstall Soloist Display with the bridge enabled and disabled, including an interrupted uninstall/rollback.
- An ARMv7 installation must refuse MPRIS without affecting Soloist Display.

## Test the installer

Use a disposable Linux user or VM. The live systemd tests can create, start and remove `soloist.service`. **Do not run them on your usual setup.**


Before a release, also test with a real Spotify Connect session:

- Fresh Managed and External installs; External → Managed migration.
- Existing custom service: either preserve its settings and session or refuse before takeover.
- Successful renewal, broken new build and interrupted renewal.
- `repair`, `doctor` and interrupted uninstall.
- Uninstall with `restore`: the old service plays again.

No green tick can replace a real speaker.
