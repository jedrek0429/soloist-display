//! Read-only diagnostics for the standalone External Soloist preview.

use crate::arguments::{DeviceCommand, GlobalFlags};
use clap::{error::ErrorKind, CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
#[cfg(any(windows, target_os = "macos"))]
use soloist_engine::release::ReleaseChannel;
use std::sync::atomic::{AtomicBool, Ordering};
static QUIET: AtomicBool = AtomicBool::new(false);
static VERBOSE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Serve,
    Launch,
    Settings,
    Doctor,
    Help,
    Version,
    Completions(Shell),
    #[cfg(any(windows, target_os = "macos"))]
    Install,
    #[cfg(any(windows, target_os = "macos"))]
    Update {
        channel: Option<ReleaseChannel>,
        version: Option<String>,
        allow_downgrade: bool,
    },
    #[cfg(any(windows, target_os = "macos"))]
    CheckUpdates,
    #[cfg(any(windows, target_os = "macos"))]
    Notifications(bool),
    #[cfg(any(windows, target_os = "macos"))]
    Repair,
    #[cfg(any(windows, target_os = "macos"))]
    Uninstall(bool),
    DeviceList,
    DeviceSet(String),
    DeviceDefault,
}

#[derive(Parser, Debug)]
#[command(
    name = "soloist-display",
    version,
    about = "Run and configure Soloist Display"
)]
struct DesktopCli {
    #[command(flatten)]
    flags: GlobalFlags,
    #[command(subcommand)]
    command: Option<DesktopSubcommand>,
}

#[derive(Subcommand, Debug)]
enum DesktopSubcommand {
    /// Open the now-playing display (starts the server if necessary).
    Launch,
    /// Open display settings (starts the server if necessary).
    Settings,
    /// Check local Soloist connectivity and audio output.
    Doctor,
    /// Choose the audio output captured by the visualiser.
    Device {
        #[command(subcommand)]
        command: DeviceCommand,
    },
    /// Generate shell completions.
    Completions { shell: Shell },
    #[cfg(any(windows, target_os = "macos"))]
    /// Install the external-only desktop application (never manages Spotify Soloist).
    Install,
    #[cfg(any(windows, target_os = "macos"))]
    /// Install a newer signed Soloist Display release.
    Update {
        #[arg(long, value_enum, conflicts_with = "version")]
        channel: Option<ReleaseChannel>,
        #[arg(long, conflicts_with = "channel")]
        version: Option<String>,
        #[arg(long)]
        allow_downgrade: bool,
    },
    #[cfg(any(windows, target_os = "macos"))]
    /// Check the latest signed release for the selected channel.
    CheckUpdates,
    #[cfg(any(windows, target_os = "macos"))]
    /// Toggle update notifications.
    Notifications {
        #[arg(value_enum)]
        setting: NotificationSetting,
    },
    #[cfg(any(windows, target_os = "macos"))]
    /// Repair the installed desktop executable and command shim.
    Repair,
    #[cfg(any(windows, target_os = "macos"))]
    /// Remove the installed Display without changing external Spotify Soloist.
    Uninstall {
        #[arg(long, value_enum, default_value_t = DataChoice::Keep)]
        data: DataChoice,
    },
}

#[cfg(any(windows, target_os = "macos"))]
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum NotificationSetting {
    On,
    Off,
}

#[cfg(any(windows, target_os = "macos"))]
#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
enum DataChoice {
    #[default]
    Keep,
    Remove,
}

pub fn parse(args: &[String]) -> Result<Command, String> {
    let parsed = DesktopCli::try_parse_from(
        std::iter::once("soloist-display").chain(args.iter().map(String::as_str)),
    );
    let cli = match parsed {
        Ok(value) => value,
        Err(error) if error.kind() == ErrorKind::DisplayHelp => return Ok(Command::Help),
        Err(error) if error.kind() == ErrorKind::DisplayVersion => return Ok(Command::Version),
        Err(error) => return Err(error.to_string()),
    };
    cli.flags.configure_color();
    QUIET.store(cli.flags.quiet, Ordering::Relaxed);
    VERBOSE.store(cli.flags.verbose, Ordering::Relaxed);
    Ok(match cli.command {
        None => Command::Serve,
        Some(DesktopSubcommand::Launch) => Command::Launch,
        Some(DesktopSubcommand::Settings) => Command::Settings,
        Some(DesktopSubcommand::Doctor) => Command::Doctor,
        Some(DesktopSubcommand::Completions { shell }) => Command::Completions(shell),
        #[cfg(any(windows, target_os = "macos"))]
        Some(DesktopSubcommand::Install) => Command::Install,
        #[cfg(any(windows, target_os = "macos"))]
        Some(DesktopSubcommand::Update {
            channel,
            version,
            allow_downgrade,
        }) => Command::Update {
            channel,
            version,
            allow_downgrade,
        },
        #[cfg(any(windows, target_os = "macos"))]
        Some(DesktopSubcommand::CheckUpdates) => Command::CheckUpdates,
        #[cfg(any(windows, target_os = "macos"))]
        Some(DesktopSubcommand::Notifications { setting }) => {
            Command::Notifications(matches!(setting, NotificationSetting::On))
        }
        #[cfg(any(windows, target_os = "macos"))]
        Some(DesktopSubcommand::Repair) => Command::Repair,
        #[cfg(any(windows, target_os = "macos"))]
        Some(DesktopSubcommand::Uninstall { data }) => {
            Command::Uninstall(matches!(data, DataChoice::Remove))
        }

        Some(DesktopSubcommand::Device { command }) => match command {
            DeviceCommand::List => Command::DeviceList,
            DeviceCommand::Use { output } | DeviceCommand::Set { output } => {
                Command::DeviceSet(output)
            }
            DeviceCommand::Default => Command::DeviceDefault,
        },
    })
}

pub fn quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

pub fn help() {
    DesktopCli::command()
        .print_help()
        .expect("stdout available");
    println!();
    println!("Device selection only changes the visualiser source, not Spotify playback.");
    println!("This build manages Soloist Display only; Spotify Soloist remains External.");
}

pub fn completions(shell: Shell) {
    let mut app = DesktopCli::command();
    generate(shell, &mut app, "soloist-display", &mut std::io::stdout());
}
