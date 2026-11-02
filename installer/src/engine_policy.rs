//! Portable policy for deciding whether Soloist Display may manage a Soloist installation.
//! OS-specific discovery and service control must implement this policy.
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineMode {
    Missing,
    External,
    Managed,
}

/// Ownership is established only by previous installer metadata and an executable
/// inside the managed root. A healthy endpoint or an executable on PATH alone
/// never establishes ownership.
pub fn classify(
    previously_managed: bool,
    executable: Option<&Path>,
    managed_root: &Path,
    endpoint_healthy: bool,
    independent_service_exists: bool,
) -> EngineMode {
    if previously_managed
        && executable.is_some_and(|path| path.is_absolute() && path.starts_with(managed_root))
    {
        return EngineMode::Managed;
    }
    if executable.is_some() || endpoint_healthy || independent_service_exists {
        EngineMode::External
    } else {
        EngineMode::Missing
    }
}

impl EngineMode {
    /// External Soloist installations must never be deleted by Display.
    pub fn may_remove_engine(self) -> bool {
        matches!(self, Self::Managed)
    }

    /// A managed engine can be installed only when no external engine exists.
    pub fn may_install_engine(self) -> bool {
        matches!(self, Self::Missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_endpoint_is_never_owned() {
        let mode = classify(false, None, Path::new("/managed"), true, false);
        assert_eq!(mode, EngineMode::External);
        assert!(!mode.may_remove_engine());
        assert!(!mode.may_install_engine());
    }

    #[test]
    fn existing_binary_is_not_owned_without_metadata() {
        let mode = classify(false, Some(Path::new("/managed/soloist")), Path::new("/managed"), false, false);
        assert_eq!(mode, EngineMode::External);
    }

    #[test]
    fn owned_binary_requires_path_containment() {
        assert_eq!(classify(true, Some(Path::new("/managed/v1/soloist")), Path::new("/managed"), false, false), EngineMode::Managed);
        assert_eq!(classify(true, Some(Path::new("/other/soloist")), Path::new("/managed"), false, false), EngineMode::External);
        assert_eq!(classify(true, Some(Path::new("soloist")), Path::new("/managed"), false, false), EngineMode::External);
    }

    #[test]
    fn absent_engine_can_be_installed() {
        let mode = classify(false, None, Path::new("/managed"), false, false);
        assert_eq!(mode, EngineMode::Missing);
        assert!(mode.may_install_engine());
        assert!(!mode.may_remove_engine());
    }

    #[test]
    fn independent_service_is_external_even_if_unhealthy() {
        assert_eq!(classify(false, None, Path::new("/managed"), false, true), EngineMode::External);
    }
}
