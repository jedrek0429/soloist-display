//! Linux CLI command grammar. Execution remains in the manager.
use crate::arguments::{DeviceCommand, GlobalFlags};
use clap::{Parser, Subcommand};
use clap_complete::Shell;
use soloist_engine::release::ReleaseChannel;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum SoloistRemoval {
    Restore,
    Keep,
    Remove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum DisplayDataRemoval {
    Keep,
    Remove,
}

#[derive(Parser, Debug)]
#[command(
    name = "soloist-display",
    version,
    about = "Install, run, and maintain Soloist Display"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    #[command(flatten)]
    pub flags: GlobalFlags,
    /// Compatibility alias for `settings`.
    #[arg(long, hide = true)]
    pub settings: bool,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum NotificationSetting {
    On,
    Off,
}

#[derive(Subcommand, Debug)]
pub enum MprisCommand {
    /// Install or start the optional MPRIS bridge.
    Enable {
        /// Soloist data directory, needed for non-default External installations.
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
    /// Stop the bridge and disable it at login without removing its files.
    Disable,
    /// Report the bridge version and service state.
    Status,
    /// Remove only the Soloist Display-owned bridge.
    Remove,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Generate shell completion script (e.g. `soloist-display completions zsh`).
    Completions { shell: Shell },
    /// Install Soloist Display with managed or independently maintained Spotify Soloist.
    Install {
        #[arg(long, hide = true, env = "SOLOIST_NAME")]
        device_name: Option<String>,
        #[arg(long, hide = true, env = "SOLOIST_API_KEY")]
        api_key: Option<String>,
        /// Use an existing Soloist instance without managing its service or binary.
        #[arg(long)]
        external_soloist: bool,
        /// WebSocket endpoint for an external Soloist instance.
        #[arg(long, requires = "external_soloist")]
        soloist_ws: Option<String>,
        #[arg(long, requires = "external_soloist")]
        start_external: bool,
    },
    /// Manage the optional Linux MPRIS bridge (media keys and playerctl).
    Mpris {
        #[command(subcommand)]
        command: MprisCommand,
    },
    /// Open the now-playing display.
    Launch,
    /// Open display settings.
    Settings,
    /// Update to the latest release on the selected channel or a specific version.
    Update {
        #[arg(long, value_enum, conflicts_with = "version")]
        channel: Option<ReleaseChannel>,
        #[arg(long, conflicts_with = "channel")]
        version: Option<String>,
        #[arg(long)]
        allow_downgrade: bool,
    },
    /// Renew the installer-owned Spotify Soloist binary from Spotify's CDN.
    RenewSoloist {
        #[arg(long)]
        force: bool,
    },
    /// Show installed version and latest available release on each channel.
    CheckUpdates,
    /// Enable or suppress update notifications without disabling manual updates.
    Notifications {
        #[arg(value_enum)]
        setting: NotificationSetting,
    },
    /// Restore installer-owned files and services.
    Repair,
    /// Check installation, services, ports, and audio.
    Doctor,
    /// Select the audio output captured by the visualiser.
    Device {
        #[command(subcommand)]
        command: DeviceCommand,
    },
    /// Safely remove installer-owned components.
    Uninstall {
        #[arg(long, value_enum)]
        soloist: Option<SoloistRemoval>,
        #[arg(long, value_enum)]
        data: Option<DisplayDataRemoval>,
    },
}
