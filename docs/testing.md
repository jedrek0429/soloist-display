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

For manual testing, run the built binaries from `target/release/` with separate configuration and ports. Check playback, display, settings, transitions, audio, and reconnection.

Do not run `install`, `update`, or `uninstall` against your regular setup. Test installer and service changes in a disposable environment.
