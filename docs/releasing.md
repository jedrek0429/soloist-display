# Releasing

Release tags use SemVer. There are two automatic prerelease lanes and one manually
approved stable lane. Release candidates are not used.

## Branches

- `alpha/vX.Y.Z`: every push publishes the next `vX.Y.Z-alpha.N`.
- `beta/vX.Y.Z`: every push publishes the next `vX.Y.Z-beta.N`.
- `main`: integration branch for fixes and approved changes; pushes do not publish.
- `stable`: protected release branch, updated only by a stable-release PR.
  Its HEAD advances when an approved release PR is merged; publication follows.

All prereleases create a versioned commit off the branch head. They do not
change their source branch. A rerun for the same source commit reuses its tag.
The shared `Signed release` pipeline builds the six Linux executables and signs
the Linux installer manifests. For alpha/beta tags, it also builds unsigned,
External-only Windows x86_64 and macOS Apple Silicon desktop previews from the
*exact tagged commit*. Both desktop builds must pass before publication, and
their ZIP archives are attached to the same GitHub Release. Stable releases do
not run desktop preview builds. A prerelease is never marked latest.

## Stable release

Run **Actions → Prepare stable release** on `main` and enter:

- **version:** `1.0.0`, `1.0.1` or `1.1.0` (no prerelease suffix).

To promote a beta, first merge `beta/vX.Y.Z` into `main` and let CI pass.
The workflow creates `release/vX.Y.Z` on top of `stable`, copies the exact
`main` tree, bumps Cargo versions and opens a PR into `stable`. This preserves
release history without reintroducing old files from `stable`.
A missing `stable` branch is bootstrapped from `main` for the first release.
The release PR shows the full difference between the last stable and `main`;
review it before merging. Incorporate any required stable-only fixes into `main` first.

Review and merge the PR only when the release is ready. The merged commit in
`stable` is tagged and the signed release pipeline is dispatched. Use squash
merge in the repository's current configuration. Never merge development PRs
directly into `stable`.

**Important:** GitHub branch protection must require reviews and checks on
`stable` and reject direct pushes. This cannot be enforced by workflow YAML
alone. Keep `stable` free of direct commits.

The `stable` ref advances on PR merge, before the GitHub Release is published.
If publication fails, rerun **Signed release** on the same tag.
Never move a published tag or replace signed assets.

## Release verification

Before stable publication, review the signing key, official Soloist download
availability, Linux architecture coverage and hardware smoke tests. CI cross-builds
are not a substitute for testing playback on real ARM hardware.

Verify release notes, the six Linux binaries, signed release and bootstrap
manifests, and the version-pinned `install.sh`. For alpha/beta, also verify
`soloist-display-preview-windows-x86_64-vX.Y.Z-CHANNEL.N.zip` and
`soloist-display-preview-macos-arm64-vX.Y.Z-CHANNEL.N.zip`, each containing
`BUILD.txt` with the exact tag and commit SHA. Publication fails if either
preview build or artifact validation fails. Test install, update, repair,
Spotify Connect, settings and uninstall on the published Linux build.

## Channels and trust

The default installer picks the latest stable release once one exists.
`SOLOIST_DISPLAY_CHANNEL=alpha` selects alpha releases, and `beta` selects beta releases.
`prerelease` remains a compatibility alias for the combined alpha/beta track.
`SOLOIST_DISPLAY_VERSION` pins an exact version. Explicit downgrades need
`--allow-downgrade`. The public GitHub Pages installer and release badge stay
on stable when alpha or beta releases are published.

Display release manifests sign and cover the Linux assets only. The experimental
Windows/macOS preview ZIPs are **unsigned** and deliberately excluded from the
Linux installer manifests; they are neither supported stable installers nor
Managed Soloist builds. Managed Soloist renewal downloads Spotify builds over
HTTPS and checks activation, but Spotify does not provide a separately
verified upstream signature. These are different trust models.
