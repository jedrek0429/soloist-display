use anyhow::{bail, Context, Result};
use std::{fs, path::Path};

pub fn set_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    let _ = path;
    Ok(())
}

pub fn set_private(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").context("USERNAME is not set")?;
        let status = std::process::Command::new("icacls")
            .arg(path)
            .args(["/inheritance:r", "/grant:r"])
            .arg(format!("{user}:(R,W)"))
            .status()
            .context("restrict credential file ACL")?;
        if !status.success() {
            bail!("could not restrict credential file ACL");
        }
    }
    let _ = path;
    Ok(())
}

pub fn is_private(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o077 == 0);
    }
    #[cfg(windows)]
    {
        let Ok(output) = std::process::Command::new("icacls").arg(path).output() else {
            return false;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        return output.status.success()
            && !text.contains("Everyone:")
            && !text.contains("BUILTIN\\Users:")
            && !text.contains("(I)");
    }
    #[allow(unreachable_code)]
    false
}

pub fn atomic_activate(target: &Path, link: &Path) -> Result<()> {
    let parent = link.parent().context("activation path has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".current.{}", std::process::id()));
    remove_link_if_present(&temporary)?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &temporary)?;
    #[cfg(windows)]
    junction::create(target, &temporary)?;

    #[cfg(unix)]
    fs::rename(&temporary, link).context("activate release")?;
    #[cfg(windows)]
    {
        let previous = parent.join(format!(".previous.{}", std::process::id()));
        remove_link_if_present(&previous)?;
        if link.exists() {
            fs::rename(link, &previous).context("stage previous activation")?;
        }
        if let Err(error) = fs::rename(&temporary, link) {
            if previous.exists() {
                let _ = fs::rename(&previous, link);
            }
            return Err(error).context("activate release");
        }
        remove_link_if_present(&previous)?;
    }
    sync_directory(parent)?;
    Ok(())
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    let _ = path;
    Ok(())
}

#[cfg(windows)]
fn is_junction(path: &Path) -> Result<bool> {
    match junction::exists(path) {
        Ok(result) => Ok(result),
        Err(error) if error.raw_os_error() == Some(4390) => Ok(false),
        Err(error) => Err(error).context("inspect Windows junction"),
    }
}

pub fn remove_link_if_present(path: &Path) -> Result<()> {
    if !path.exists() && fs::symlink_metadata(path).is_err() {
        return Ok(());
    }
    #[cfg(windows)]
    if is_junction(path)? {
        junction::delete(path)?;
        return Ok(());
    }
    if fs::symlink_metadata(path)?.file_type().is_symlink() || path.is_file() {
        fs::remove_file(path)?;
    } else if path.is_dir() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

/// Resolve both paths before deciding ownership. This deliberately fails for
/// missing files and for symlinks/junctions that escape the managed root.
pub fn canonical_contained(path: &Path, root: &Path) -> bool {
    if !path.is_absolute() || !path.is_file() {
        return false;
    }
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    path.canonicalize()
        .is_ok_and(|resolved| resolved.starts_with(root))
}

pub fn safe_remove(path: &Path, allowed_roots: &[&Path]) -> Result<()> {
    if !path.is_absolute() {
        bail!("refusing to remove relative path {}", path.display());
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if !is_managed_path(path, allowed_roots) {
        bail!("refusing to remove unmanaged path {}", path.display());
    }
    #[cfg(windows)]
    if is_junction(path)? {
        junction::delete(path)?;
        return Ok(());
    }
    if metadata.file_type().is_symlink() || path.is_file() {
        fs::remove_file(path)?;
    } else if path.is_dir() {
        let resolved = path.canonicalize()?;
        if !allowed_roots.iter().any(|root| {
            root.canonicalize()
                .is_ok_and(|allowed| resolved.starts_with(allowed))
        }) {
            bail!("refusing to remove escaped directory {}", path.display());
        }
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

pub fn is_managed_path(path: &Path, allowed_roots: &[&Path]) -> bool {
    if !path.is_absolute() {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(parent) = parent.canonicalize() else {
        return false;
    };
    allowed_roots.iter().any(|root| {
        root.canonicalize()
            .is_ok_and(|root| parent.starts_with(root))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_and_outside_removal() {
        let root = tempfile::tempdir().unwrap();
        assert!(safe_remove(Path::new("relative"), &[root.path()]).is_err());
        let outside = tempfile::tempdir().unwrap();
        let file = outside.path().join("keep");
        fs::write(&file, b"keep").unwrap();
        assert!(safe_remove(&file, &[root.path()]).is_err());
        assert!(file.exists());
    }

    #[cfg(unix)]
    #[test]
    fn canonical_ownership_rejects_symlink_escape() {
        let managed = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let binary = outside.path().join("soloist");
        fs::write(&binary, b"external").unwrap();
        let link = managed.path().join("soloist");
        std::os::unix::fs::symlink(&binary, &link).unwrap();
        assert!(!canonical_contained(&link, managed.path()));
    }
}
