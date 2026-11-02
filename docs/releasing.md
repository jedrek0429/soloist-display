# Signed releases

Tags matching the workspace version (`vX.Y.Z`) build static manager and audio-bridge executables for x86_64, ARM64, and ARMv7. The release workflow downloads the current official Spotify Soloist archives, records their digests in the application manifest, signs both manifests with Ed25519, and publishes the artifacts through GitHub Releases.

The committed public key is the trust anchor. Its private key must exist only as the protected `RELEASE_SIGNING_KEY` GitHub Actions secret. Generate a replacement offline with `openssl genpkey -algorithm ED25519`; update the embedded public key in both `install.sh` and `installer/src/release.rs` in a reviewed key-rotation release. Never commit, print, or upload the private key as an artifact.

Release tags are immutable. A correction requires a new version and tag. Pull-request checks compile every target; x86_64 tests run natively, while ARM artifacts are cross-compiled and inspected. A release must not claim native ARM runtime validation unless it was performed separately and recorded in the release notes.
