mod dependencies;
mod lock;
mod manager;
mod migrations;
mod paths;
mod platform;
mod process;
mod release;
mod service;
mod soloist;
mod state;

use anyhow::Result;
use clap::{Parser, Subcommand};
use lock::InstallLock;
use manager::RemoveMode;
use paths::Paths;
use platform::Platform;
use std::{
    fs,
    io::{IsTerminal, Write},
    os::unix::fs::OpenOptionsExt,
    time::Duration,
};

#[derive(Parser, Debug)]
#[command(
    name = "soloist-display",
    version,
    about = "Install, run, and maintain Soloist Display"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Accept safe defaults and run without confirmation.
    #[arg(long, global = true)]
    yes: bool,
    /// Disable ANSI styling.
    #[arg(long, global = true)]
    no_color: bool,
    /// Show only essential output.
    #[arg(long, global = true)]
    quiet: bool,
    /// Show additional diagnostics.
    #[arg(long, global = true)]
    verbose: bool,
    /// Compatibility alias for `settings`.
    #[arg(long, hide = true)]
    settings: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Install Soloist Display and, when absent, Spotify Soloist.
    Install {
        #[arg(long, hide = true, env = "SOLOIST_NAME")]
        device_name: Option<String>,
        #[arg(long, hide = true, env = "SOLOIST_API_KEY")]
        api_key: Option<String>,
    },
    /// Open the now-playing display.
    Launch,
    /// Open display settings.
    Settings,
    /// Install the latest signed release.
    Update,
    /// Restore installer-owned files and services.
    Repair,
    /// Check installation, services, ports, and audio.
    Doctor,
    /// Safely remove installer-owned components.
    Uninstall {
        #[arg(long, value_enum)]
        mode: Option<RemoveMode>,
    },
}

fn locked<T>(paths: &Paths, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    paths.ensure_management_dirs()?;
    let _lock = InstallLock::acquire(&paths.lock, Duration::from_secs(15))?;
    operation()
}

fn run() -> Result<i32> {
    let cli = Cli::parse();
    if cli.no_color {
        std::env::set_var("NO_COLOR", "1");
    }
    let paths = Paths::discover()?;
    let requested = if cli.settings {
        Some(Command::Settings)
    } else {
        cli.command
    };
    let command = match requested {
        Some(command) => command,
        None if state::InstallState::load(&paths)?.is_some() => Command::Launch,
        None => Command::Install {
            device_name: None,
            api_key: None,
        },
    };
    match command {
        Command::Install {
            device_name,
            api_key,
        } => {
            let platform = Platform::detect()?;
            locked(&paths, || {
                manager::install(&paths, &platform, cli.yes, cli.quiet, device_name, api_key)
            })?;
        }
        Command::Launch => locked(&paths, || manager::launch(&paths, false))?,
        Command::Settings => locked(&paths, || manager::launch(&paths, true))?,
        Command::Update => {
            let platform = Platform::detect()?;
            locked(&paths, || manager::update(&paths, &platform, cli.quiet))?;
        }
        Command::Repair => {
            let platform = Platform::detect()?;
            locked(&paths, || manager::repair(&paths, &platform, cli.quiet))?;
        }
        Command::Doctor => {
            if !manager::doctor(&paths)? {
                return Ok(1);
            }
        }
        Command::Uninstall { mode } => locked(&paths, || manager::uninstall(&paths, mode))?,
    }
    Ok(0)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("soloist-display: {error:#}");
        if let Ok(paths) = Paths::discover() {
            if fs::create_dir_all(&paths.logs).is_ok() {
                let log = paths.logs.join("installer.log");
                if let Ok(mut file) = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .mode(0o600)
                    .open(&log)
                {
                    let _ = writeln!(file, "{}: {error:#}", state::now());
                    eprintln!("Details: {}", log.display());
                }
            }
        }
        if std::io::stderr().is_terminal() {
            eprintln!("Run `soloist-display doctor` for diagnostics.");
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    #[test]
    fn cli_is_consistent() {
        Cli::command().debug_assert();
    }
}
