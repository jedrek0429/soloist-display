//! One command grammar for Soloist Display across Linux, Windows and macOS.
use crate::arguments::{DeviceCommand, GlobalFlags};
use clap::{Parser, Subcommand};
use clap_complete::Shell;
use soloist_display_core::release::ReleaseChannel;
#[cfg(target_os = "linux")]
use std::path::PathBuf;

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
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
    #[cfg(target_os = "linux")]
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
    #[cfg(target_os = "linux")]
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
        #[cfg(target_os = "linux")]
        #[arg(long, value_enum)]
        soloist: Option<SoloistRemoval>,
        #[arg(long, value_enum)]
        data: Option<DisplayDataRemoval>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    #[test]
    fn common_commands_have_one_valid_grammar() {
        Cli::command().debug_assert();
        let cli = Cli::try_parse_from(["soloist-display", "device", "use", "2"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Device { .. })));
        let cli = Cli::try_parse_from(["soloist-display", "update", "--channel", "alpha"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Update { .. })));
        let cli = Cli::try_parse_from(["soloist-display", "uninstall", "--data", "keep"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Uninstall { .. })));
        assert!(Cli::try_parse_from(["soloist-display", "--verbose", "--quiet"]).is_err());
    }

    #[test]
    fn shared_global_flags_retain_settings_alias() {
        let cli = Cli::try_parse_from(["soloist-display", "--settings"]).unwrap();
        assert!(cli.settings);
        assert!(cli.command.is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_only_managed_commands_use_common_parser() {
        assert!(matches!(
            Cli::try_parse_from(["soloist-display", "renew-soloist", "--force"])
                .unwrap()
                .command,
            Some(Command::RenewSoloist { force: true })
        ));
        assert!(Cli::try_parse_from(["soloist-display", "mpris", "status"]).is_ok());
    }
}
