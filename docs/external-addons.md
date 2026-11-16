# External add-ons

Soloist Display owns add-on installation state and user-facing lifecycle commands.
External binaries do not need a Soloist Display plugin SDK or lifecycle protocol.

## Distribution contract

Each supported add-on has a trusted release definition inside Soloist Display:

- GitHub repository and pinned release version
- release archive filename for each supported Linux architecture
- expected executable path inside the archive
- maximum archive and executable sizes

The shared release downloader (`crates/installer/src/addons/release.rs`) downloads
`https://github.com/<owner>/<repo>/releases/download/v<version>/<asset>`
and the companion `<asset>.sha256` file. It verifies that the checksum names
exactly the requested asset, validates its SHA-256 digest, and checks ELF
architecture before returning executable bytes. The add-on integration stages
those bytes in the managed installation directory.

A GitHub repository alone is not enough to install an add-on: the asset format,
checksums and supported architectures must be established and tested before
adding its release definition.

## Lifecycle

The Soloist Display CLI owns `addon list`, `addon status`, `addon enable`,
`addon disable` and `addon remove`. The installer handles lock management,
recovery and uninstall transaction staging. Each integration currently owns
only the operations that actually differ between applications.

- **MPRIS:** Installs the unchanged upstream `soloist-mpris` executable,
  a systemd user unit, and a configured Soloist data directory.
- **Firewall (future):** A standalone `soloist-firewall` executable should
  manage its own firewall-specific behavior. Its Soloist Display integration
  must specify how configuration, privileges and cleanup work before any
  firewall rule is changed.

The add-on executable is **not required** to implement `activate`,
`deactivate`, `health`, or any other standardized plugin hooks.

## Requirements for soloist-firewall

Build and test `soloist-firewall` independently first. Before integrating it:

1. Publish versioned release archives and SHA-256 companion files.
2. Document the supported Linux architectures and exact executable path.
3. Define how it identifies the Soloist ZeroConf port and owns its UFW rules.
4. Define narrowly scoped privilege escalation and safe removal of owned rules.
5. Do not enable UFW, change default policies or touch SSH/unowned rules.
6. Provide a reliable way for the installer to stop/reconcile/clean up its
   resources during disable, remove and interrupted uninstall.

Only then add a release definition and a minimal lifecycle integration to
Soloist Display. Do not register an unavailable or hypothetical add-on.
