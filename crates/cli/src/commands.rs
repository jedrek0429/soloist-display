//! One command grammar for Soloist Display across Linux, Windows and macOS.
use crate::arguments::{DeviceCommand, GlobalFlags};
use clap::{Parser, Subcommand};
use clap_complete::Shell;
use soloist_display_core::release::ReleaseChannel;
#[cfg(target_os = "linux")]
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
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum NotificationSetting {
    On,
    Off,
}

#[cfg(target_os = "linux")]
#[derive(Subcommand, Debug)]
pub enum AddonCommand {
    /// List available add-ons.
    List,

    /// Show the status of an add-on.
    Status { id: String },

    /// Install if necessary and enable an add-on.
    Enable,

    /// Disable an add-on without removing it.
    Disable { id: String },

    /// Uninstall an add-on and remove its managed files.
    Remove { id: String },
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
    /// Open the now-playing display.
    Launch,
    /// Open display settings.
    Settings,
    /// Edit the installation configuration in $EDITOR.
    #[cfg(target_os = "linux")]
    Config,
    /// Validate and reload the installation configuration.
    #[cfg(target_os = "linux")]
    Reload {
        /// Preview changes without restarting services.
        #[arg(long)]
        dry_run: bool,
    },
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
    /// Manage optional add-ons.
    #[cfg(target_os = "linux")]
    Addon {
        #[command(subcommand)]
        command: AddonCommand,
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
    fn only_the_settings_subcommand_opens_settings() {
        assert!(Cli::try_parse_from(["soloist-display", "--settings"]).is_err());
        assert!(matches!(
            Cli::try_parse_from(["soloist-display", "settings"])
                .unwrap()
                .command,
            Some(Command::Settings)
        ));
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
        assert!(Cli::try_parse_from(["soloist-display", "config"]).is_ok());
        assert!(Cli::try_parse_from(["soloist-display", "reload", "--dry-run"]).is_ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_only_addon_commands_parse() {
        assert!(Cli::try_parse_from(["soloist-display", "addon", "list",]).is_ok());

        assert!(Cli::try_parse_from(["soloist-display", "addon", "enable",]).is_ok());

        for action in ["status", "disable", "remove"] {
            assert!(Cli::try_parse_from(["soloist-display", "addon", action, "mpris",]).is_ok());
        }
    }
}
