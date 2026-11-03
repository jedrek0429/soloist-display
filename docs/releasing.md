# Releasing

Tags follow [SemVer](https://semver.org/) and match the workspace Cargo version: `v1.0.0-alpha.1`, `v1.0.0-beta.1`, `v1.0.0-rc.1`, `v1.0.0`.

Published tags are immutable. Need a fix? Publish a new tag. Don't rewrite history.

## Prepare a release

Open **Actions → Prepare release → Run workflow** on `main`. Enter a SemVer version such as `1.0.0-beta.1`, `1.0.0-rc.1` or `1.0.0` (the `v` prefix is optional).

The workflow validates the version, updates `Cargo.toml` and `Cargo.lock`, and opens `chore: bump vX.Y.Z`. It also dispatches the build and distribution checks on that branch. GitHub suppresses ordinary workflow triggers from changes made using `GITHUB_TOKEN`.

Review the PR and its checks before merging. Merging the version PR into `main` creates an annotated tag for the merged commit and dispatches **Signed release**. That existing workflow builds binaries, signs manifests and publishes the release. Prereleases are not marked as the latest stable release.

GitHub Actions must be allowed to create pull requests under **Settings → Actions → General → Workflow permissions**. Alternatively, set `RELEASE_BOT_TOKEN` to a token with Contents and Pull requests write access. `RELEASE_SIGNING_KEY` must be configured for the signing job. No private signing key belongs in the repository.

If automatic publication fails after the tag was created, rerun **Signed release** with its `vX.Y.Z` tag as the ref. Do not move a published tag or silently replace release assets.

## Manual fallback

Publish manually only if the automation cannot run. Use the version in `Cargo.toml`, update `Cargo.lock`, merge the change and push a new annotated `vX.Y.Z` tag. A manually pushed tag triggers **Signed release**.

Before any release, verify the signing key matches the public keys embedded in `install.sh` and `installer/src/release.rs`, check that Spotify Soloist archives are available, and review supported Linux distributions and known limitations. CI cross-builds do not prove native ARM playback. See [installer tests](testing.md) for the manual hardware and migration checks.

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
