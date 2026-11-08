# Architecture (pre-10)

Soloist Display is one Rust workspace shared across Linux, Windows and macOS. Prefer platform-independent code; isolate native OS behavior behind small adapters. Spotify Soloist is a separate playback engine.

## Workspace

| Crate directory and package | Responsibility |
| --- | --- |
| `crates/soloist-display-cli` | One clap command grammar, flags, shell completions and CLI output |
| `crates/soloist-display-core` | Signed releases, ownership, state persistence, transaction journals, rollback and cleanup |
| `crates/soloist-display-installer` | Lifecycle orchestration; Linux Managed service integration and desktop External-only installer |
| `crates/soloist-display-audio` | CPAL system-output capture, PCM/FFT, HTTP/WebSocket Display runtime |
| `crates/soloist-display-platform` | Linux distro/ABI and XDG, Windows AppData and macOS filesystem paths |

The crate directory and Cargo package names are identical. The existing executable names (`soloist-display` and `soloist-audio-bridge`) remain unchanged to preserve signed artifact and service compatibility.

## Shared lifecycle invariants

- A single CLI grammar defines common operations. Platform capabilities may gate execution, never silently pretend to support Managed Soloist.
- `core::release` verifies signed Ed25519 manifests and SHA-256 artifacts without an unsigned fallback.
- `core::lifecycle` implements versioned state reads, atomic writes, typed journals, staged-file rename, reverse-order rollback and committed cleanup.
- Linux journals include systemd/Managed recovery data. Desktop activation journals capture prior installer-owned launcher and state bytes, and replay after interruption.
- Recovered or newly discovered Spotify Soloist processes, services, executables and sessions are never considered installer-owned merely because they exist.
- The platform layer contains only OS-dependent integration. Audio stays on CPAL 0.18.2; microphone input is not a fallback for output monitoring.

## Limits before phases III–IV

Windows/macOS remain External-only previews until Managed installation and native process supervision are delivered. Linux's existing Managed installation and service behavior must remain intact. Autostart and background supervision belong to stage 10; native media integration to stage 11; External auto-discovery to stage 12; desktop Managed lifecycle and renewal to stages 13–15.

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
```

Use Linux systemd integration and glibc checks plus native Windows/macOS tests. Successful compilation does not substitute for real signed upgrade/rollback, process crash and uninstall testing on each platform.
