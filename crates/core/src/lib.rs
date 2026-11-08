//! Ownership rules shared by platform-specific Soloist lifecycle implementations.
//! Discovery is not ownership: finding an executable, service, or WebSocket
//! must never grant permission to replace or uninstall that Soloist instance.

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineMode {
    Missing,
    External,
    Managed,
}

impl EngineMode {
    pub fn may_remove_engine(self) -> bool {
        self == Self::Managed
    }
}

/// An installed path is owned only if explicit metadata says so and the
/// resolved executable is a file physically inside the installer-owned root.
/// Canonicalisation prevents following a symlink out into an external install.
pub fn owns_executable(path: &Path, managed_root: &Path) -> bool {
    if !path.is_absolute() || !managed_root.is_absolute() {
        return false;
    }
    let Ok(metadata) = std::fs::symlink_metadata(managed_root) else {
        return false;
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return false;
    }
    let (Ok(path), Ok(root)) = (path.canonicalize(), managed_root.canonicalize()) else {
        return false;
    };
    path.is_file() && path.starts_with(root)
}

/// Derive the observed mode. This does not install, start, stop or mutate any
/// Soloist process or file. Mutating operations must independently verify
/// ownership again, rather than trusting this discovery snapshot.
pub fn classify(
    previously_managed: bool,
    executable: Option<&Path>,
    managed_root: &Path,
    endpoint_reachable: bool,
    independent_service_exists: bool,
) -> EngineMode {
    if previously_managed && executable.is_some_and(|path| owns_executable(path, managed_root)) {
        EngineMode::Managed
    } else if executable.is_some() || endpoint_reachable || independent_service_exists {
        EngineMode::External
    } else {
        EngineMode::Missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn external_endpoint_is_not_owned() {
        let mode = classify(false, None, Path::new("/unused"), true, false);
        assert_eq!(mode, EngineMode::External);
        assert!(!mode.may_remove_engine());
    }

    #[test]
    fn external_service_is_not_owned() {
        let mode = classify(false, None, Path::new("/unused"), false, true);
        assert_eq!(mode, EngineMode::External);
        assert!(!mode.may_remove_engine());
    }

    #[test]
    fn missing_engine_has_no_ownership() {
        assert_eq!(
            classify(false, None, Path::new("/unused"), false, false),
            EngineMode::Missing
        );
    }

    #[test]
    fn managed_binary_needs_prior_ownership_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("managed");
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("soloist");
        fs::write(&executable, b"managed").unwrap();

        assert_eq!(
            classify(false, Some(&executable), &root, false, false),
            EngineMode::External
        );
        assert_eq!(
            classify(true, Some(&executable), &root, false, false),
            EngineMode::Managed
        );
        assert!(EngineMode::Managed.may_remove_engine());
    }

    #[test]
    fn lookalike_path_outside_root_is_not_owned() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("managed");
        let other = dir.path().join("managed-other");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&other).unwrap();
        let executable = other.join("soloist");
        fs::write(&executable, b"external").unwrap();
        assert_eq!(
            classify(true, Some(&executable), &root, false, false),
            EngineMode::External
        );
    }

    #[test]
    fn vanished_managed_executable_is_not_owned() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("managed");
        fs::create_dir_all(&root).unwrap();
        assert_eq!(
            classify(true, Some(&root.join("missing")), &root, false, false),
            EngineMode::External
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_out_of_managed_root_is_external() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("managed");
        fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("outside-soloist");
        fs::write(&outside, b"external").unwrap();
        let link = root.join("soloist");
        symlink(&outside, &link).unwrap();
        assert_eq!(
            classify(true, Some(&link), &root, false, false),
            EngineMode::External
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_managed_root_does_not_claim_external_installation() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let external = dir.path().join("external-install");
        fs::create_dir_all(&external).unwrap();
        let executable = external.join("soloist");
        fs::write(&executable, b"external").unwrap();
        let managed_root = dir.path().join("managed");
        symlink(&external, &managed_root).unwrap();
        assert_eq!(
            classify(
                true,
                Some(&managed_root.join("soloist")),
                &managed_root,
                false,
                false
            ),
            EngineMode::External
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_into_managed_root_is_managed() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("managed");
        let version = root.join("versions").join("v1");
        fs::create_dir_all(&version).unwrap();
        let executable = version.join("soloist");
        fs::write(&executable, b"managed").unwrap();
        let current = root.join("current");
        symlink(&version, &current).unwrap();
        assert_eq!(
            classify(true, Some(&current.join("soloist")), &root, false, false),
            EngineMode::Managed
        );
    }
}
