# Automatic Spotify Soloist binary renewal

## Current limitation

Soloist Display installs the Spotify Soloist executable from a signed Soloist Display release manifest. Updating Display only replaces the managed Soloist binary when the Display version changes, and the signed manifest can continue to reference an expired Soloist artifact. A scheduled invocation of `soloist-display update` alone does not solve this.

## Required implementation

- Introduce a dedicated `soloist-display renew-soloist` command, separate from Display version updates.
- Discover the currently published, supported official Soloist binary independently of Display releases. Confirm the official distribution and artifact metadata contract before implementation; do not guess a URL.
- Verify authenticity and integrity before activation. Preserve the existing signed-manifest trust model or establish an equally strong documented verification chain.
- Record the installation/expiry date when authoritative metadata is available. Check daily and renew at least seven days before expiry. Handle exit code 10 as an urgent recovery signal, not an endless restart loop.
- Use a systemd user timer for installer-owned Soloist only. Preserve independent installations without modifications.
- Stage and validate the executable, switch the managed symlink atomically, restart the service safely, verify WebSocket health, and roll back on failure.
- Avoid interrupting active playback when possible, but provide bounded recovery for an already expired executable.
- Preserve API keys, session data, configuration and device name. Do not print secrets.
- Add tests for expiry detection, renewal selection, invalid artifacts, failed downloads, rollback, independent ownership, timer installation and uninstall cleanup.

## Acceptance criteria

An installer-owned Soloist instance remains functional past the expiry of the initially installed executable without requiring a new Soloist Display release. A failed renewal retains or restores the last working executable and reports an actionable error. An independently installed Soloist remains untouched.

This document tracks the design; **the renewal implementation is not yet present on this branch**.

## Reference implementation: MopigamesYT/soloistd

Source: https://github.com/MopigamesYT/soloistd (MPL-2.0; use the design, do not copy source without complying with the license).

The upstream official CDN archive is fetched from `https://soloist-builds.spotifycdn.com/` plus the platform filename:

- x86_64: `soloist_release_x86_64.tar.gz`
- AArch64: `soloist_release_arm64.tar.gz`
- ARMv7: `soloist_release_arm32.tar.gz`

`soloistd/download.go` sends HEAD and compares ETag, then X-Amz-Checksum-Crc32c, then Last-Modified plus Content-Length to locally recorded metadata. If the remote archive changed, it downloads the archive, extracts only a regular `soloist` file, runs the staged binary with `--version` (15-second timeout, expecting a `soloist ` prefix), and atomically renames the replacement. Its supervisor checks every six hours and treats exit status 10 as an unconditional redownload trigger. Proactive network failures do not interrupt playback.

### Adaptation for Soloist Display

Use the existing Rust manager and installer-owned systemd service; do not add Go or a dependency on soloistd. Prefer a six-hour systemd user timer for proactive checks and a recovery path for code 10. Keep the managed versioned directories and symlink activation/rollback semantics, existing configuration, and independence safeguards.

**Security caveat:** ETag and Last-Modified identify CDN object changes but are not cryptographic authenticity proofs. The CRC32C response header is not a signature. Verify HTTPS origin, bound archive and extracted sizes, validate architecture and `--version`, and retain an independently verifiable integrity/authenticity mechanism wherever available. Do not silently describe CDN metadata as a signed release.

Reference: https://github.com/MopigamesYT/soloistd/blob/main/download.go and https://github.com/MopigamesYT/soloistd/blob/main/supervisor.go.
