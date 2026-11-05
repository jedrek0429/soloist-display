# Releasing

Release tags use SemVer. There are two automatic prerelease lanes and one manually
approved stable lane. Release candidates are not used.

## Branches

- `alpha/vX.Y.Z`: every push publishes the next `vX.Y.Z-alpha.N`.
- `beta/vX.Y.Z`: every push publishes the next `vX.Y.Z-beta.N`.
- `main`: integration branch for fixes and approved changes; pushes do not publish.
- `stable`: protected release branch, updated only by a stable-release PR.
  Its HEAD is the code of the latest stable tag after publication succeeds.

All prereleases create a versioned commit off the branch head. They do not
change their source branch. A rerun for the same source commit reuses its tag.
The shared `Signed release` pipeline builds and signs the release. A prerelease
is never marked latest.

## Stable release

Run **Actions → Prepare stable release** on `main` and enter:

- **version:** `1.0.0`, `1.0.1` or `1.1.0` (no prerelease suffix).
- **target_branch:** source branch to promote, normally `main`; an `alpha/vX.Y.Z`
  or `beta/vX.Y.Z` branch may also be selected.

The workflow creates `release/vX.Y.Z` from the selected source, integrates the
previous `stable` history, bumps Cargo versions and opens a PR into `stable`.
A missing `stable` branch is bootstrapped from `main` for the first release.
Integration conflicts stop the workflow; never silently discard stable fixes.

Review and merge the PR only when the release is ready. The merged commit in
`stable` is tagged and the signed release pipeline is dispatched. Use squash
merge in the repository's current configuration. Never merge development PRs
directly into `stable`.

**Important:** GitHub branch protection must require reviews and checks on
`stable` and reject direct pushes. This cannot be enforced by workflow YAML
alone. Keep `stable` free of direct commits.

If the tagged release job fails, rerun **Signed release** on the same tag.
Never move a published tag or replace signed assets.

## Release verification

Before stable publication, review the signing key, official Soloist download
availability, Linux architecture coverage and hardware smoke tests. CI cross-builds
are not a substitute for testing playback on real ARM hardware.

Verify release notes, the six Linux binaries, signed release and bootstrap
manifests, and the version-pinned `install.sh`. Test install, update, repair,
Spotify Connect, settings and uninstall on the published build.

## Channels and trust

The default installer picks the latest stable release once one exists.
`SOLOIST_DISPLAY_CHANNEL=prerelease` selects the highest SemVer prerelease;
`SOLOIST_DISPLAY_VERSION` pins an exact version. Explicit downgrades need
`--allow-downgrade`. The public GitHub Pages installer and release badge stay
on stable when alpha or beta releases are published.

Display release manifests are signed. Managed Soloist renewal downloads Spotify
builds over HTTPS and checks activation, but Spotify does not provide a
separately verified upstream signature. These are different trust models.
