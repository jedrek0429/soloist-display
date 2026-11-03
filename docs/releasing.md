# Releasing Soloist Display

Soloist Display uses [Semantic Versioning](https://semver.org/). Release tags are immutable and match the workspace Cargo version exactly: `v1.0.0-alpha.1`, `v1.0.0-beta.1`, `v1.0.0-rc.1`, then `v1.0.0`. Never reuse a published tag: publish `alpha.2` to correct `alpha.1`.

## Channels and updates

The default installer selects the highest published stable SemVer release. Until the first stable release exists, it selects the highest published prerelease. Publishing an older tag cannot change the preferred release. Once stable exists, users opt in to prereleases explicitly.

The installed channel is persisted in installation metadata, including across updates to a stable version. Older metadata infers the initial channel from its installed version. A stable installation never automatically opts into prereleases.

```sh
soloist-display check-updates
soloist-display update
soloist-display update --channel prerelease
soloist-display update --channel stable
soloist-display update --version 1.2.0-rc.3
soloist-display update --version 1.0.0-alpha.1 --allow-downgrade
soloist-display notifications off
soloist-display notifications on
```

Updates are never installed unattended. On an interactive launch, the CLI may offer `Update now? [y/N]`; only an explicit `y` starts the update. In non-interactive sessions it shows a notice without installing. Notification suppression persists and disables the prompt, but does not prevent an explicit check or update. The prerelease channel remains selected when an RC upgrades to its stable release.

Explicit versions take priority over channels; selecting a version persists its corresponding stable/prerelease channel. Changing channels does not authorize a downgrade. A downgrade requires `--allow-downgrade` or `SOLOIST_DISPLAY_ALLOW_DOWNGRADE=1`. Configuration migrations may not be reversible, so back up the settings before downgrading.

`SOLOIST_DISPLAY_VERSION` and `SOLOIST_DISPLAY_CHANNEL` remain available as environment overrides. Releases and downloaded binaries are verified against signed manifests; GitHub release metadata is only used to discover candidates.

## Prerequisites

- Merged, reviewed changes on `main`; all required checks green.
- Protected `RELEASE_SIGNING_KEY` secret configured. This is an Ed25519 PEM private key; never commit, print, or upload it. The matching embedded public key in `install.sh` and `installer/src/release.rs` is the trust anchor.
- Available upstream Spotify Soloist archives for x86_64, arm64 and arm32; Rust, Zig and GitHub Actions release permissions.
- Confirm Linux and glibc requirements, known limitations, and changes from the previous release.

## Preflight

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

Test `soloist-display --version`, `doctor`, `repair`, `update`, `uninstall`, fresh install, old-settings migration, interrupted update and rollback in an isolated installation. Check all supported architectures. CI cross-compilation and file-format checks are not evidence of native ARM execution; record separately any hardware runtime testing.

## Version bump

Set `[workspace.package] version = "1.0.0-alpha.1"` in `Cargo.toml`, synchronize `Cargo.lock` with Cargo, review the diff, and re-run the preflight. The tag validation step compares `v`-stripped tag against the installer crate's version. Update README, release notes and screenshots if behavior changed. Merge the preparation PR before tagging.

## Create and publish

```sh
git switch main
git pull --ff-only
git status --short             # must be clean
git tag -a v1.0.0-alpha.1 -m "Soloist Display v1.0.0-alpha.1"
git push origin v1.0.0-alpha.1
```

The `Signed release` workflow builds a single statically linked `release-tools` executable for CI signing and publishing tasks, along with static application binaries for x86_64, ARM64 and ARMv7, downloads and validates official Soloist archives, hashes the assets, signs the application and bootstrap manifests with Ed25519, renders the release-bound `install.sh`, and publishes the GitHub Release. A tag with a prerelease suffix is automatically marked **Pre-release** and `--latest=false`. Stable tags publish without this flag. Never publish a release if any signing/build step failed.

## Verify after publication

1. In GitHub Releases verify the tag, Pre-release badge, title and notes.
2. Verify six architecture-specific binaries, `release-manifest.json`, `release-manifest.sig`, `bootstrap-manifest.txt`, `bootstrap-manifest.sig` and rendered `install.sh`. Verify SHA-256 and signatures before use.
3. Test a fresh install using the **specific published** bootstrap: `curl -fsSL https://github.com/jedrek0429/soloist-display/releases/download/v1.0.0-alpha.1/install.sh | SOLOIST_DISPLAY_VERSION=1.0.0-alpha.1 sh`.
4. Check `soloist-display --version`, settings, playback, `doctor`, `update`, `repair` and rollback. Validate the GitHub Pages version link and installer file.
5. Record native-platform test coverage, compatibility issues and known limitations in release notes.

## Failure and recovery

Do not force-push or retag an already published release. Fix the underlying issue, bump to the next prerelease number, open a PR, merge, and publish a new tag. If signing fails, investigate the secret, PEM formatting and public-key match without exposing the private key. Failed verification must remain fail-closed. If a published release is unsafe, document the problem and direct users to a verified newer release; do not silently replace signed assets.

A key rotation requires generating a new Ed25519 pair offline (`openssl genpkey -algorithm ED25519`), reviewing and updating **both** embedded public keys in one coordinated release, and changing the protected GitHub Actions secret. Never store the private key in the repository.

## Release checklist

- [ ] SemVer version and lockfile match
- [ ] Formatting, tests, clippy, documentation and distribution checks pass
- [ ] Signing secret configured; public keys match
- [ ] Alpha/stable channel behavior tested
- [ ] Fresh install, update, migration, repair and rollback tested
- [ ] Tag created from merged `main`
- [ ] Release signed and marked correctly
- [ ] GitHub Pages installer and version verified
- [ ] Release notes include supported platforms and known limitations
