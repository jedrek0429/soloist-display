//! Linux-only target selection around the common signed-release verifier.
pub use soloist_engine::release::*;
use crate::platform::Platform;
use anyhow::{anyhow, Result};

/// Linux supports legacy musl manifest aliases while migrating to glibc.
/// Selection remains a thin adapter; signatures and downloads are shared.
pub trait LinuxReleaseArtifact {
    fn artifact<'a>(&'a self, kind: &str, platform: &Platform) -> Result<&'a Artifact>;
}

impl LinuxReleaseArtifact for ReleaseManifest {
    fn artifact<'a>(&'a self, kind: &str, platform: &Platform) -> Result<&'a Artifact> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.kind == kind && artifact.target == platform.target)
            .or_else(|| {
                (platform.target == platform.architecture.release_target()).then_some(())?;
                self.artifacts.iter().find(|artifact| {
                    artifact.kind == kind
                        && artifact.target == platform.architecture.legacy_target()
                })
            })
            .ok_or_else(|| {
                anyhow!(
                    "release {} has no {kind} artifact for {}",
                    self.version,
                    platform.target
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{Architecture, Distro};
    use semver::Version;

    #[test]
    fn gnu_manager_can_select_previous_signed_musl_artifacts() {
        use crate::platform::{Architecture, Distro};
        let platform = Platform {
            distro: Distro::Ubuntu,
            architecture: Architecture::X86_64,
            target: "x86_64-unknown-linux-gnu".into(),
            label: "test".into(),
        };
        let legacy = Artifact {
            name: "soloist-display-x86_64-unknown-linux-musl".into(),
            kind: "manager".into(),
            target: "x86_64-unknown-linux-musl".into(),
            url: "https://example.invalid/legacy".into(),
            fallback_url: None,
            sha256: "0".repeat(64),
            size: 1,
        };
        let mut manifest = ReleaseManifest {
            schema: 1,
            version: Version::new(1, 0, 0),
            published_at: String::new(),
            artifacts: vec![legacy.clone()],
        };
        assert_eq!(
            manifest.artifact("manager", &platform).unwrap().target,
            legacy.target
        );
        manifest.artifacts.push(Artifact {
            target: platform.target.clone(),
            ..legacy
        });
        assert_eq!(
            manifest.artifact("manager", &platform).unwrap().target,
            platform.target
        );
        assert!(manifest.artifact("audio-bridge", &platform).is_err());
    }
}
