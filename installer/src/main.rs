mod dependencies;
mod lock;
mod manager;
mod migrations;
mod paths;
mod platform;
mod process;
mod release;
mod renewal;
mod service;
mod soloist;
mod state;
mod terminal;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use lock::InstallLock;
use manager::RemoveMode;
use paths::Paths;
use platform::Platform;
use release::ReleaseChannel;
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

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum NotificationSetting {
    On,
    Off,
}

#[derive(Subcommand, Debug)]
enum Command {
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
    terminal::configure(cli.quiet, cli.verbose);
    if cli.no_color {
        std::env::set_var("NO_COLOR", "1");
    }
    if let Some(Command::Completions { shell }) = &cli.command {
        generate(
            *shell,
            &mut Cli::command(),
            "soloist-display",
            &mut std::io::stdout(),
        );
        return Ok(0);
    }
    let paths = Paths::discover()?;
    terminal::set_log_path(paths.logs.join("installer.jsonl"));
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
            external_soloist: false,
            soloist_ws: None,
        },
    };
    match command {
        Command::Install {
            device_name,
            api_key,
            external_soloist,
            soloist_ws,
        } => {
            let platform = Platform::detect()?;
            locked(&paths, || {
                manager::install(
                    &paths,
                    &platform,
                    cli.yes,
                    cli.quiet,
                    manager::InstallOptions {
                        name: device_name,
                        api_key,
                        external_soloist,
                        soloist_ws,
                    },
                )
            })?;
        }
        Command::Launch => manager::launch(&paths, false)?,
        Command::Settings => manager::launch(&paths, true)?,
        Command::Completions { .. } => unreachable!("handled before path discovery"),
        Command::Update {
            channel,
            version,
            allow_downgrade,
        } => {
            let platform = Platform::detect()?;
            locked(&paths, || {
                manager::update(
                    &paths,
                    &platform,
                    cli.quiet,
                    channel,
                    version.as_deref(),
                    allow_downgrade,
                )
            })?;
        }
        Command::RenewSoloist { force } => {
            let platform = Platform::detect()?;
            locked(&paths, || renewal::renew(&paths, &platform, force))?;
        }
        Command::CheckUpdates => manager::update_status(&paths)?,
        Command::Notifications { setting } => {
            locked(&paths, || {
                manager::set_update_notifications(
                    &paths,
                    matches!(setting, NotificationSetting::On),
                )
            })?;
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
        Command::Uninstall { mode } => {
            locked(&paths, || manager::uninstall(&paths, mode, cli.quiet))?
        }
    }
    Ok(0)
}

fn main() {
    match run() {
        Ok(code) => {
            if code != 0 {
                std::process::exit(code);
            }
        }
        Err(error) => {
            terminal::log_event("installer", "error", &format!("{error:#}"));
            let plain = std::env::var_os("NO_COLOR").is_some()
                || !std::io::stderr().is_terminal()
                || std::env::var("TERM").as_deref() == Ok("dumb");
            if plain {
                eprintln!("  error  {error:#}");
            } else {
                eprintln!("  \x1b[31m✗\x1b[0m {error:#}");
            }
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
                        eprintln!("  Log: {}", log.display());
                    }
                }
            }
            if terminal::verbose() {
                let executable = std::env::current_exe().ok();
                if let Some(executable) = executable {
                    eprintln!("Diagnostics: {} doctor", executable.display());
                }
            }
            std::process::exit(1);
        }
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
