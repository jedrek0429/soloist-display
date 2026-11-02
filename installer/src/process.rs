use anyhow::{bail, Context, Result};
use std::{
    env,
    path::Path,
    process::{Command, Stdio},
};

pub fn exists(name: &str) -> bool {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 {
        return candidate.is_file();
    }
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|directory| {
            let plain = directory.join(name);
            if plain.is_file() {
                return true;
            }
            #[cfg(windows)]
            return env::var_os("PATHEXT").is_some_and(|extensions| {
                extensions
                    .to_string_lossy()
                    .split(';')
                    .any(|extension| directory.join(format!("{name}{extension}")).is_file())
            });
            #[cfg(not(windows))]
            false
        })
    })
}

pub fn run(command: &mut Command, description: &str, quiet: bool) -> Result<()> {
    // Keep the interactive UI clean; diagnostics remain in the returned error.
    let _ = quiet;
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let status = command.status().with_context(|| description.to_owned())?;
    if !status.success() {
        bail!("{description} failed with {status}");
    }
    Ok(())
}
