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

## Test the installer

Use a disposable Linux user or VM. The live systemd tests can create, start and remove `soloist.service`. **Do not run them on your usual setup.**

CI runs the usual Rust, shell and frontend checks for relevant PRs. Generated unit files are checked with `systemd-analyze verify` in the normal Rust tests.

The existing **Installer integration** job runs only when installer-related files change (or when dispatched manually). It uses a real systemd user manager to start a generated Soloist service with a fake executable and test credentials. It checks that `EnvironmentFile` supplies the device name and API key. No Spotify account, audio device or network download is needed.

**Signed release** reuses that same integration job and will not sign or publish until it passes. Its separate signed lifecycle fixture covers install, repair, update and uninstall. Cross-building ARM binaries is not the same as testing on ARM hardware.

Before a release, also test with a real Spotify Connect session:

- Fresh Managed and External installs; External → Managed migration.
- Existing custom service: either preserve its settings and session or refuse before takeover.
- Successful renewal, broken new build and interrupted renewal.
- `repair`, `doctor` and interrupted uninstall.
- Uninstall with `restore`: the old service plays again.

No green tick can replace a real speaker.
