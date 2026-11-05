# Releasing

Release tags use SemVer. Alpha and beta builds are automatic; stable releases
are explicitly dispatched. Release candidates are not used.

## Branches

- `main`: development source and development Cargo version. Releases never modify it.
- `alpha/vX.Y.Z`: each push publishes the next `vX.Y.Z-alpha.N`.
- `beta/vX.Y.Z`: each push publishes the next `vX.Y.Z-beta.N`.
- `stable`: exact commit of the latest successfully published stable release.

Each release has its own versioned commit, built from the selected source commit.
`Cargo.toml` and `Cargo.lock` are changed only in that release commit.
Prerelease branches and `main` keep their own versions.

## Stable release

First merge the intended code into `main` and let Checks pass. Promote
beta changes into `main` before publishing stable.

Run **Actions → Prepare stable release** on `main`, entering a stable version
such as `1.0.0`. The workflow checks the requested version against both
`main` and the previous `stable` version, creates a versioned commit based
on `main`, and tests that commit. It then creates an immutable tag and
dispatches **Signed release**.

Only after the signed GitHub Release has been published does **Signed release**
move `stable` to the exact tag commit. The move uses a SHA lease and refuses
to replace `stable` if another release became latest in the meantime.
The release tag and `stable` should resolve to the same commit after success.

`stable` is a moving pointer, not an integration branch. Its history may be
rewritten when a new stable tag is published, because each release commit
descends from a `main` source commit, not from the previous `stable` commit.
Do not open PRs into `stable`. Never merge `stable` back into `main`.

### Required GitHub configuration

The `stable protect` ruleset must grant the release automation identity a
**bypass** of its PR requirement, required checks and non-fast-forward rules
for `stable`. Keep everyone else protected. Configure a GitHub App (preferred)
or a dedicated automation identity for the bypass. Supply its credential
as the repository secret `RELEASE_BOT_TOKEN` (Contents: read/write).
The workflow uses this credential only to update `stable`.

Without a matching bypass, the release is still published but the stable
synchronisation job fails. Do not disable protection globally as a workaround.
After fixing permissions, rerun the failed `sync-stable` job for the published
tag. Do not rerun release creation or move a published tag.

## Release verification

Before stable publication, review the signing key, official Soloist download
availability, Linux architecture coverage and hardware smoke tests. CI cross-builds
are not a substitute for testing playback on real ARM hardware.

Verify release notes, the six Linux binaries, signed release and bootstrap
manifests, and the version-pinned `install.sh`. Test install, update, repair,
Spotify Connect, settings and uninstall on the published build.

## Channels and trust

The default installer picks the latest stable release once one exists.
`SOLOIST_DISPLAY_CHANNEL=alpha` selects alpha releases, and `beta` selects beta releases.
`prerelease` remains a compatibility alias for the combined alpha/beta track.
`SOLOIST_DISPLAY_VERSION` pins an exact version. Explicit downgrades need
`--allow-downgrade`. The public GitHub Pages installer and release badge stay
on stable when alpha or beta releases are published.

Display release manifests are signed. Managed Soloist renewal downloads Spotify
builds over HTTPS and checks activation, but Spotify does not provide a
separately verified upstream signature. These are different trust models.
