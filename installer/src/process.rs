use anyhow::{bail, Context, Result};
use std::process::{Command, Stdio};

pub fn exists(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", "command -v \"$1\" >/dev/null 2>&1", "sh", name])
        .status()
        .is_ok_and(|status| status.success())
}

pub fn run(command: &mut Command, description: &str, quiet: bool) -> Result<()> {
    if quiet {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let status = command.status().with_context(|| description.to_owned())?;
    if !status.success() {
        bail!("{description} failed with {status}");
    }
    Ok(())
}
