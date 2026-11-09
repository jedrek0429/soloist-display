use anyhow::{anyhow, Context, Result};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
    thread,
    time::{Duration, Instant},
};

pub struct InstallLock {
    file: File,
}

impl InstallLock {
    pub fn acquire(path: &Path, timeout: Duration) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .with_context(|| format!("open installer lock {}", path.display()))?;
        let start = Instant::now();
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(Self { file }),
                Err(error) if start.elapsed() < timeout => {
                    let _ = error;
                    thread::sleep(Duration::from_millis(200));
                }
                Err(_) => {
                    return Err(anyhow!(
                        "another Soloist Display operation holds {} (waited {}s)",
                        path.display(),
                        timeout.as_secs()
                    ))
                }
            }
        }
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_installer_is_rejected_and_stale_file_is_safe() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("installer.lock");
        let first = InstallLock::acquire(&path, Duration::from_millis(10)).unwrap();
        assert!(InstallLock::acquire(&path, Duration::from_millis(20)).is_err());
        drop(first);
        InstallLock::acquire(&path, Duration::from_millis(20)).unwrap();
    }
}
