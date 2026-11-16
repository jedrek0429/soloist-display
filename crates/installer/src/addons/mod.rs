use crate::paths::Paths;
use crate::platform::Platform;
use anyhow::Result;
use clap::{builder::PossibleValuesParser, Command};
use std::path::PathBuf;

pub(crate) mod mpris;
mod release;
use mpris::MprisAddon;

#[derive(Debug, Clone, Copy)]
pub struct AddonMetadata {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone)]
pub struct AddonStatus {
    pub installation: AddonInstallState,
    pub enabled: bool,
    pub running: Option<bool>,
    pub version: Option<String>,
    pub details: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct AddonUninstallPlan {
    pub files: Vec<PathBuf>,
    pub services: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddonInstallState {
    NotInstalled,
    Pending,
    Installed,
}

pub trait Addon: Sync {
    fn metadata(&self) -> AddonMetadata;

    // Generic add-on commands
    fn status(&self, paths: &Paths) -> Result<AddonStatus>;
    fn enable(&self, paths: &Paths, platform: &Platform) -> Result<()>;
    fn disable(&self, paths: &Paths) -> Result<()>;
    fn remove(&self, paths: &Paths) -> Result<()>;

    // Lifecycle methods
    fn recover(&self, paths: &Paths) -> Result<()>;
    fn uninstall_plan(&self, paths: &Paths) -> Result<Option<AddonUninstallPlan>>;
    fn stop_for_uninstall(&self, paths: &Paths) -> Result<()>;

    // Optional enable-command definition with additional arguments
    fn enable_command(&self) -> clap::Command {
        clap::Command::new(self.metadata().id)
    }

    fn enable_with(
        &self,
        paths: &Paths,
        platform: &Platform,
        _matches: &clap::ArgMatches,
    ) -> Result<()> {
        self.enable(paths, platform)
    }

}

static MPRIS: MprisAddon = MprisAddon;

pub static ADDONS: &[&dyn Addon] = &[&MPRIS];

pub fn find(id: &str) -> Option<&'static dyn Addon> {
    ADDONS
        .iter()
        .copied()
        .find(|addon| addon.metadata().id == id)
}

pub fn list() {
    for addon in ADDONS {
        let metadata = addon.metadata();

        println!(
            "{}: {} — {}",
            metadata.id, metadata.name, metadata.description
        );
    }
}

pub fn status(id: &str, paths: &Paths) -> Result<()> {
    let addon = find(id).ok_or_else(|| anyhow::anyhow!("Unknown add-on '{id}'"))?;

    let metadata = addon.metadata();
    let status = addon.status(paths)?;

    println!(
        "{}: {}",
        metadata.name,
        match status.installation {
            AddonInstallState::NotInstalled => "not installed",
            AddonInstallState::Pending => "incomplete",
            AddonInstallState::Installed => "installed",
        }
    );

    if let Some(version) = status.version {
        println!("Version: {version}");
    }

    if status.installation != AddonInstallState::NotInstalled {
        println!("Service enabled: {}", status.enabled);

        if let Some(running) = status.running {
            println!("Service active: {running}");
        }

        println!("Add-on details:");

        for (key, value) in status.details {
            println!("{key}: {value}");
        }
    }

    Ok(())
}

pub fn disable(id: &str, paths: &Paths) -> Result<()> {
    let addon = find(id).ok_or_else(|| anyhow::anyhow!("Unknown add-on '{id}'"))?;

    addon.disable(paths)
}

pub fn remove(id: &str, paths: &Paths) -> Result<()> {
    let addon = find(id).ok_or_else(|| anyhow::anyhow!("Unknown add-on '{id}'"))?;

    addon.remove(paths)
}

pub fn configure_cli(mut command: Command) -> Command {
    let ids: Vec<&'static str> = ADDONS.iter().map(|addon| addon.metadata().id).collect();

    command = command.mut_subcommand("addon", |addon| {
        ["disable", "status", "remove"]
            .into_iter()
            .fold(addon, |addon, action| {
                addon.mut_subcommand(action, |subcommand| {
                    subcommand.mut_arg("id", |arg| {
                        arg.value_parser(PossibleValuesParser::new(ids.clone()))
                    })
                })
            })
    });

    command = command.mut_subcommand("addon", |addon| {
        addon.mut_subcommand("enable", |mut enable| {
            for addon in ADDONS {
                enable = enable.subcommand(addon.enable_command());
            }

            enable.subcommand_required(true)
        })
    });

    command
}

pub fn recover_all(paths: &Paths) -> Result<()> {
    for addon in ADDONS {
        addon.recover(paths)?;
    }

    Ok(())
}

pub fn uninstall_plans(paths: &Paths) -> Result<Vec<(&'static str, AddonUninstallPlan)>> {
    let mut plans = Vec::new();

    for addon in ADDONS {
        if let Some(plan) = addon.uninstall_plan(paths)? {
            plans.push((addon.metadata().id, plan));
        }
    }

    Ok(plans)
}

pub fn stop_all_for_uninstall(paths: &Paths) -> Result<()> {
    for addon in ADDONS {
        if addon.uninstall_plan(paths)?.is_some() {
            addon.stop_for_uninstall(paths)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::addons::{configure_cli, uninstall_plans};
    use crate::test_support::paths;
    use clap::CommandFactory;
    use soloist_display_cli::linux::Cli;

    #[test]
    fn no_addons_installed_means_no_uninstall_plans() {
        let (_directory, paths) = paths();

        let plans = uninstall_plans(&paths).unwrap();

        assert!(plans.is_empty());
    }

    #[test]
    fn mpris_enable_rejects_unknown_arguments() {
        assert!(configure_cli(Cli::command())
            .try_get_matches_from([
                "soloist-display",
                "addon",
                "enable",
                "mpris",
                "--invalid-option",
            ])
            .is_err());
    }

    #[test]
    fn addon_enable_accepts_mpris_options() {
        let matches = configure_cli(Cli::command())
            .try_get_matches_from([
                "soloist-display",
                "addon",
                "enable",
                "mpris",
                "--data-dir",
                "/tmp/soloist-test",
            ])
            .unwrap();

        let options = matches
            .subcommand_matches("addon")
            .unwrap()
            .subcommand_matches("enable")
            .unwrap()
            .subcommand_matches("mpris")
            .unwrap();

        assert_eq!(
            options.get_one::<std::path::PathBuf>("data-dir"),
            Some(&std::path::PathBuf::from("/tmp/soloist-test"))
        );
    }

    #[test]
    fn addon_cli_rejects_invalid_commands() {
        for arguments in [
            vec!["addon", "enable", "nonexistent"],
            vec!["addon", "status", "nonexistent"],
            vec!["addon", "enable", "mpris", "--unknown"],
            vec!["mpris", "status"],
        ] {
            let result = configure_cli(Cli::command())
                .try_get_matches_from(std::iter::once("soloist-display").chain(arguments));

            assert!(result.is_err());
        }
    }
}
