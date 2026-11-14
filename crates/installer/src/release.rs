//! Linux-only target selection around the common signed-release verifier.
use crate::platform::Platform;
use anyhow::Result;
pub use soloist_display_core::release::*;

/// Select the exact GNU/Linux target; signatures and downloads are shared.
pub trait LinuxReleaseArtifact {
    fn artifact<'a>(&'a self, kind: &str, platform: &Platform) -> Result<&'a Artifact>;
}

impl LinuxReleaseArtifact for ReleaseManifest {
    fn artifact<'a>(&'a self, kind: &str, platform: &Platform) -> Result<&'a Artifact> {
        self.artifact_target(kind, &platform.target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

    #[test]
    fn linux_artifact_requires_exact_target() {
        use crate::platform::{Architecture, Distro};
        let platform = Platform {
            distro: Distro::Ubuntu,
            architecture: Architecture::X86_64,
            target: "x86_64-unknown-linux-gnu".into(),
            label: "test".into(),
        };
        let manifest = ReleaseManifest {
            schema: 1,
            version: Version::new(1, 0, 0),
            published_at: String::new(),
            artifacts: vec![Artifact {
                name: "soloist-display-x86_64-unknown-linux-gnu".into(),
                kind: "manager".into(),
                target: platform.target.clone(),
                url: "https://example.invalid/gnu".into(),
                fallback_url: None,
                sha256: "0".repeat(64),
                size: 1,
            }],
        };
        assert_eq!(
            manifest.artifact("manager", &platform).unwrap().target,
            platform.target
        );
        assert!(manifest.artifact("audio-bridge", &platform).is_err());
        let mut wrong_target = manifest.clone();
        wrong_target.artifacts[0].target = "x86_64-unknown-linux-musl".into();
        assert!(wrong_target.artifact("manager", &platform).is_err());
    }
}
