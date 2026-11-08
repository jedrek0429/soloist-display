//! Desktop runtime command mapping onto the shared CLI grammar.
use crate::{
    arguments::DeviceCommand,
    commands::{Cli, Command as SharedCommand},
};
use clap::{error::ErrorKind, CommandFactory, Parser};
use clap_complete::{generate, Shell};
#[cfg(any(windows, target_os = "macos"))]
use crate::commands::{DisplayDataRemoval, NotificationSetting};
#[cfg(any(windows, target_os = "macos"))]
use soloist_display_core::release::ReleaseChannel;
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

pub fn parse(args: &[String]) -> Result<Command, String> {
    let parsed = Cli::try_parse_from(
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
    if cli.settings {
        return Ok(Command::Settings);
    }
    Ok(match cli.command {
        None => Command::Serve,
        Some(SharedCommand::Launch) => Command::Launch,
        Some(SharedCommand::Settings) => Command::Settings,
        Some(SharedCommand::Doctor) => Command::Doctor,
        Some(SharedCommand::Completions { shell }) => Command::Completions(shell),
        #[cfg(any(windows, target_os = "macos"))]
        Some(SharedCommand::Install {
            device_name,
            api_key,
            external_soloist: _,
            soloist_ws,
            start_external,
        }) => {
            if device_name.is_some() || api_key.is_some() || soloist_ws.is_some() || start_external
            {
                return Err("Managed Soloist and external process control are unavailable in this desktop preview".into());
            }
            Command::Install
        }
        #[cfg(target_os = "linux")]
        Some(SharedCommand::Install { .. }) => {
            return Err("install must be invoked through the installed Linux manager".into());
        }
        #[cfg(any(windows, target_os = "macos"))]
        Some(SharedCommand::Update {
            channel,
            version,
            allow_downgrade,
        }) => Command::Update {
            channel,
            version,
            allow_downgrade,
        },
        #[cfg(any(windows, target_os = "macos"))]
        Some(SharedCommand::CheckUpdates) => Command::CheckUpdates,
        #[cfg(any(windows, target_os = "macos"))]
        Some(SharedCommand::Notifications { setting }) => {
            Command::Notifications(matches!(setting, NotificationSetting::On))
        }
        #[cfg(any(windows, target_os = "macos"))]
        Some(SharedCommand::Repair) => Command::Repair,
        #[cfg(any(windows, target_os = "macos"))]
        Some(SharedCommand::Uninstall { data, .. }) => {
            Command::Uninstall(matches!(data, Some(DisplayDataRemoval::Remove)))
        }
        #[cfg(target_os = "linux")]
        Some(
            SharedCommand::Update { .. }
            | SharedCommand::CheckUpdates
            | SharedCommand::Notifications { .. }
            | SharedCommand::Repair
            | SharedCommand::Uninstall { .. }
            | SharedCommand::Mpris { .. }
            | SharedCommand::RenewSoloist { .. },
        ) => {
            return Err(
                "lifecycle management must be invoked through the installed Linux manager".into(),
            );
        }
        Some(SharedCommand::Device { command }) => match command {
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
    Cli::command().print_help().expect("stdout available");
    println!();
    println!("Device selection only changes the visualiser source, not Spotify playback.");
    println!("This build manages Soloist Display only; Spotify Soloist remains External.");
}

pub fn completions(shell: Shell) {
    let mut app = Cli::command();
    generate(shell, &mut app, "soloist-display", &mut std::io::stdout());
}
