use anyhow::{bail, Context, Result};
use std::{
    env,
    path::Path,
    process::{Command, Stdio},
};

pub fn exists(name: &str) -> bool {
    find(name).is_some()
}

pub fn find(name: &str) -> Option<std::path::PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }
    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path).find_map(|directory| {
            let plain = directory.join(name);
            if plain.is_file() {
                return Some(plain);
            }
            #[cfg(windows)]
            return env::var_os("PATHEXT").and_then(|extensions| {
                extensions
                    .to_string_lossy()
                    .split(';')
                    .find_map(|extension| {
                        let path = directory.join(format!("{name}{extension}"));
                        path.is_file().then_some(path)
                    })
            });
            #[cfg(not(windows))]
            None
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
