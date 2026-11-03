# Releasing

Tags follow [SemVer](https://semver.org/) and match the workspace Cargo version: `v1.0.0-alpha.1`, `v1.0.0-beta.1`, `v1.0.0-rc.1`, `v1.0.0`.

Published tags are immutable. Need a fix? Publish a new tag. Don't rewrite history.

## Before tagging

- Merge reviewed changes into `main`; all checks must pass.
- Set the workspace version and update `Cargo.lock`.
- Check the protected `RELEASE_SIGNING_KEY` (Ed25519 PEM) matches the public key embedded in `install.sh` and `installer/src/release.rs`. Never print the private key.
- Check upstream Spotify Soloist archives for x86_64, ARM64 and ARMv7 are available.
- Check supported Linux distributions and known limitations.

```sh
git switch main && git pull --ff-only
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
sh -n install.sh
cargo run --quiet -p soloist-display-installer --bin release-tools -- check-settings .
cargo run --quiet -p soloist-display-installer --bin release-tools -- validate-tag v1.0.0-alpha.1
```

Run [installer tests](testing.md) in isolation. Verify Managed and External, migration, renewal, `repair`, `doctor`, rollback and all uninstall choices. Test a real Spotify session where possible. CI cross-builds do not prove native ARM playback.

## Publish

```sh
git status --short
git tag -a v1.0.0-alpha.1 -m "Soloist Display v1.0.0-alpha.1"
git push origin v1.0.0-alpha.1
```

The `Signed release` workflow builds the application for x86_64, ARM64 and ARMv7, verifies release artefacts, signs the release and bootstrap manifests with Ed25519, and publishes the GitHub Release. Prereleases are marked as such and are not set as the latest stable release.

Never publish if signing or any build failed.

## Check the release

1. Verify the tag, release notes and prerelease status.
2. Check the six architecture-specific binaries, `release-manifest.json`, `release-manifest.sig`, `bootstrap-manifest.txt`, `bootstrap-manifest.sig` and `install.sh`.
3. Install the **exact published release** in a clean environment:

   ```sh
   curl -fsSL https://github.com/jedrek0429/soloist-display/releases/download/v1.0.0-alpha.1/install.sh | SOLOIST_DISPLAY_VERSION=1.0.0-alpha.1 sh
   ```

4. Check `--version`, Spotify Connect, settings, `doctor`, `update`, `renew-soloist`, `repair` and `uninstall`.
5. Check the GitHub Pages version link and bootstrap. Record hardware test coverage and known faults.

## Channels and trust

The default installer chooses the highest stable release, or the highest prerelease until the first stable exists. Stable users do not enter the prerelease channel automatically. An explicit version wins over a channel; downgrades need `--allow-downgrade`. `SOLOIST_DISPLAY_VERSION` and `SOLOIST_DISPLAY_CHANNEL` remain available.

Display releases use signed manifests. Periodic **Spotify Soloist** renewal is separate: it uses Spotify's HTTPS CDN, not an independently signed upstream manifest. Do not describe those checks as equivalent.

If a release is bad, document it and ship a new, signed version. Do not replace published assets silently.

Key rotation means generating a new Ed25519 pair offline, updating both embedded public keys and replacing the protected signing secret together. Never commit the private key.
