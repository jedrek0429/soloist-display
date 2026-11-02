use anyhow::{bail, Context, Result};
use std::process::Command;

pub fn exists(name: &str) -> bool {
    Command::new("sh")
        .args(["-c", "command -v \"$1\" >/dev/null 2>&1", "sh", name])
        .status()
        .is_ok_and(|status| status.success())
}

pub fn run(command: &mut Command, description: &str, _quiet: bool) -> Result<()> {
    let output = command.output().with_context(|| description.to_owned())?;
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    crate::terminal::log_event(description, if output.status.success() { "success" } else { "failure" }, &format!("exit: {}; stderr: {}", output.status, diagnostic.chars().take(8192).collect::<String>()));
    crate::terminal::log_command(description, &output.stdout, &output.stderr);
    if !output.status.success() {
        let reason = diagnostic.trim();
        if reason.is_empty() {
            bail!("{description} failed with {}", output.status);
        }
        bail!("{description} failed with {}: {}", output.status, reason);
    }
    Ok(())
}
