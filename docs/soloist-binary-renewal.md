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
