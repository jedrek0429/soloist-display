//! Command-line options shared by the installed Linux manager and portable desktop bridge.
use clap::{Args, Subcommand};

#[derive(Args, Debug, Clone, Default)]
pub struct GlobalFlags {
    /// Accept safe defaults without interactive confirmation.
    #[arg(long, global = true)]
    pub yes: bool,
    /// Disable ANSI colors.
    #[arg(long, global = true)]
    pub no_color: bool,
    /// Suppress nonessential output.
    #[arg(long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,
    /// Display additional diagnostics.
    #[arg(long, global = true, conflicts_with = "quiet")]
    pub verbose: bool,
}

impl GlobalFlags {
    pub fn configure_color(&self) {
        if self.no_color {
            std::env::set_var("NO_COLOR", "1");
        }
    }
}

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum DeviceCommand {
    /// List available audio outputs and stable identifiers.
    List,
    /// Select visualiser output by number, stable ID or unique name.
    Use {
        #[arg(value_parser = clap::builder::NonEmptyStringValueParser::new())]
        output: String,
    },
    /// Alias for 'use'.
    Set {
        #[arg(value_parser = clap::builder::NonEmptyStringValueParser::new())]
        output: String,
    },
    /// Follow the system default audio output.
    Default,
}
