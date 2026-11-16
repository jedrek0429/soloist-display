use soloist_display_cli::{arguments as cli_arguments, output as cli_output};
mod addons;
mod config;
mod dependencies;
mod legacy;
mod lock;
mod manager;
mod paths;
mod platform;
mod process;
mod release;
mod renewal;
mod service;
mod soloist;
mod state;
mod terminal;
mod uninstall_tx;

use anyhow::{bail, Result};
use clap::{CommandFactory, FromArgMatches};
use clap_complete::generate;
use cli_arguments::DeviceCommand;
use lock::InstallLock;
use manager::UninstallOptions;
use paths::Paths;
use platform::Platform;
use soloist_display_cli::linux::{AddonCommand, Cli, Command, NotificationSetting};
use std::{
    fs,
    io::{IsTerminal, Write},
    os::unix::fs::OpenOptionsExt,
    time::Duration,
};

fn locked<T>(paths: &Paths, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    paths.ensure_management_dirs()?;
    let _lock = InstallLock::acquire(&paths.lock, Duration::from_secs(15))?;
    uninstall_tx::recover(paths)?;
    manager::recover(paths)?;
    renewal::recover(paths)?;
    addons::recover_all(paths)?;
    operation()
}

fn run() -> Result<i32> {
    let matches = addons::configure_cli(Cli::command()).get_matches();

    // Handle add-on optional enable-command definitions
    if let Some(("addon", addon_matches)) = matches.subcommand() {
        if let Some(("enable", enable_matches)) = addon_matches.subcommand() {
            let (id, options) = enable_matches.subcommand().expect("an add-on is required");

            let addon = addons::find(id).expect("Clap accepted an unregistered add-on");

            let mut root_matches = matches.clone();
            root_matches.remove_subcommand();

            let cli = Cli::from_arg_matches(&root_matches)?;

            terminal::configure(cli.flags.quiet, cli.flags.verbose);
            cli.flags.configure_color();

            let paths = Paths::discover()?;
            let platform = Platform::detect()?;

            locked(&paths, || addon.enable_with(&paths, &platform, options))?;

            return Ok(0);
        }
    }

    // Handle add-on optional CLI command definitions
    if let Some(addon) = matches.subcommand_name().and_then(addons::command_owner) {
        let mut root_matches = matches.clone();
        let (_, addon_matches) = root_matches
            .remove_subcommand()
            .expect("registered add-on command");

        let cli = Cli::from_arg_matches(&root_matches)?;

        terminal::configure(cli.flags.quiet, cli.flags.verbose);
        cli.flags.configure_color();

        let paths = Paths::discover()?;

        locked(&paths, || addon.run_command(&addon_matches, &paths))?;

        return Ok(0);
    }

    let cli = Cli::from_arg_matches(&matches)?;
    terminal::configure(cli.flags.quiet, cli.flags.verbose);
    cli.flags.configure_color();
    if let Some(Command::Completions { shell }) = &cli.command {
        let mut command = addons::configure_cli(Cli::command());

        generate(
            *shell,
            &mut command,
            "soloist-display",
            &mut std::io::stdout(),
        );

        return Ok(0);
    }

    let paths = Paths::discover()?;
    if uninstall_tx::pending(&paths) {
        locked(&paths, || Ok(()))?;
    }
    terminal::set_log_path(paths.logs.join("installer.jsonl"));
    let command = match cli.command {
        Some(command) => command,
        None if state::InstallState::load(&paths)?.is_some() => Command::Launch,
        None => Command::Install {
            device_name: None,
            api_key: None,
            external_soloist: false,
            soloist_ws: None,
            start_external: false,
        },
    };
    match command {
        Command::Install {
            device_name,
            api_key,
            external_soloist,
            soloist_ws,
            start_external,
        } => {
            let platform = Platform::detect()?;
            locked(&paths, || {
                manager::install(
                    &paths,
                    &platform,
                    cli.flags.yes,
                    cli.flags.quiet,
                    manager::InstallOptions {
                        name: device_name,
                        api_key,
                        external_soloist,
                        soloist_ws,
                        start_external,
                    },
                )
            })?;
            config::ensure(&paths)?;
        }
        Command::Addon { command } => match command {
            AddonCommand::List => addons::list(),
            AddonCommand::Status { id } => addons::status(&id, &paths)?,
            AddonCommand::Enable => unreachable!("handled during CLI parsing"),
            AddonCommand::Disable { id } => {
                locked(&paths, || addons::disable(&id, &paths))?;
            }
            AddonCommand::Remove { id } => {
                locked(&paths, || addons::remove(&id, &paths))?;
            }
        },
        Command::Launch => manager::launch(&paths, false)?,
        Command::Settings => manager::launch(&paths, true)?,
        Command::Config => config::edit(&paths)?,
        Command::Reload { dry_run } => locked(&paths, || config::reload(&paths, dry_run))?,
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
                    cli.flags.quiet,
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
            locked(&paths, || {
                manager::repair(&paths, &platform, cli.flags.quiet)
            })?;
        }
        Command::Doctor => {
            if !manager::doctor(&paths)? {
                return Ok(1);
            }
        }
        Command::Device { command } => {
            let executable = paths.current.join("bin/soloist-audio-bridge");
            if !executable.is_file() {
                bail!("Display audio bridge is missing; run 'soloist-display repair'");
            }
            let mut process = std::process::Command::new(&executable);
            process.arg("device");
            match command {
                DeviceCommand::List => {
                    process.arg("list");
                }
                DeviceCommand::Use { output } => {
                    process.arg("use").arg(output);
                }
                DeviceCommand::Set { output } => {
                    process.arg("set").arg(output);
                }
                DeviceCommand::Default => {
                    process.arg("default");
                }
            }
            let status = process.status()?;
            if !status.success() {
                return Ok(status.code().unwrap_or(1));
            }
        }
        Command::Uninstall { soloist, data } => locked(&paths, || {
            manager::uninstall(
                &paths,
                UninstallOptions {
                    soloist,
                    data,
                    yes: cli.flags.yes,
                },
                cli.flags.quiet,
            )
        })?,
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
mod test_support;

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    #[test]
    fn cli_is_consistent() {
        Cli::command().debug_assert();
    }
}
