use crate::platform::Platform;
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use clap::ValueEnum;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use reqwest::blocking::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

pub const RELEASE_PUBLIC_KEY_B64: &str = "N2qrYATzPFCel3Nw7NZHzZtba1Gz9kthPRXWtQMFTiw=";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Artifact {
    pub name: String,
    pub kind: String,
    pub target: String,
    pub url: String,
    #[serde(default)]
    pub fallback_url: Option<String>,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub schema: u32,
    pub version: Version,
    pub published_at: String,
    pub artifacts: Vec<Artifact>,
}

impl ReleaseManifest {
    pub fn artifact<'a>(&'a self, kind: &str, platform: &Platform) -> Result<&'a Artifact> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.kind == kind && artifact.target == platform.target)
            .ok_or_else(|| {
                anyhow!(
                    "release {} has no {kind} artifact for {}",
                    self.version,
                    platform.target
                )
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseChannel {
    Stable,
    Alpha,
    Beta,
    /// Legacy combined channel, kept for existing installations.
    Prerelease,
}

impl ReleaseChannel {
    pub fn for_version(version: &Version) -> Self {
        if version.pre.is_empty() {
            Self::Stable
        } else if version.pre.as_str().starts_with("alpha.") {
            Self::Alpha
        } else if version.pre.as_str().starts_with("beta.") {
            Self::Beta
        } else {
            Self::Prerelease
        }
    }

    fn accepts(self, version: &Version) -> bool {
        match self {
            Self::Stable => version.pre.is_empty(),
            Self::Alpha => version.pre.as_str().starts_with("alpha."),
            Self::Beta => version.pre.as_str().starts_with("beta."),
            Self::Prerelease => !version.pre.is_empty(),
        }
    }
}

pub fn select_release_tag(
    releases: &[serde_json::Value],
    channel: Option<ReleaseChannel>,
) -> Result<String> {
    let versions = releases.iter().filter_map(|release| {
        if release["draft"].as_bool() != Some(false) {
            return None;
        }
        let tag = release["tag_name"].as_str()?;
        let parsed = Version::parse(tag.strip_prefix('v')?).ok()?;
        if format!("v{parsed}") != tag || !parsed.build.is_empty() {
            return None;
        }
        if !parsed.pre.is_empty() {
            let prerelease = parsed.pre.as_str();
            if !(prerelease.starts_with("alpha.") || prerelease.starts_with("beta.")) {
                return None;
            }
        }
        Some((parsed, tag))
    });
    let mut available: Vec<_> = versions.collect();
    let stable_exists = available.iter().any(|(v, _)| v.pre.is_empty());
    match channel {
        Some(channel) => available.retain(|(version, _)| channel.accepts(version)),
        None if stable_exists => available.retain(|(version, _)| version.pre.is_empty()),
        None => {}
    }
    available
        .into_iter()
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, tag)| tag.to_owned())
        .ok_or_else(|| anyhow!("no published release for selected channel"))
}

pub struct ReleaseClient {
    client: Client,
    public_key: VerifyingKey,
}

impl ReleaseClient {
    pub fn new() -> Result<Self> {
        let key: [u8; 32] = STANDARD
            .decode(RELEASE_PUBLIC_KEY_B64)?
            .try_into()
            .map_err(|_| anyhow!("invalid embedded release public key"))?;
        let public_key = VerifyingKey::from_bytes(&key)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .user_agent(format!("soloist-display/{}", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { client, public_key })
    }

    pub fn latest(&self) -> Result<(ReleaseManifest, Vec<u8>)> {
        self.select(None, None)
    }

    pub fn select(
        &self,
        channel: Option<ReleaseChannel>,
        requested_version: Option<&str>,
    ) -> Result<(ReleaseManifest, Vec<u8>)> {
        if let Some(path) = std::env::var_os("SOLOIST_DISPLAY_MANIFEST") {
            let bytes = fs::read(&path)
                .with_context(|| format!("read manifest {}", Path::new(&path).display()))?;
            let signature_path = std::env::var_os("SOLOIST_DISPLAY_MANIFEST_SIGNATURE")
                .ok_or_else(|| {
                    anyhow!("SOLOIST_DISPLAY_MANIFEST_SIGNATURE is required with a local manifest")
                })?;
            let signature = fs::read(signature_path)?;
            return self.verify_manifest(bytes, signature);
        }
        let requested = requested_version
            .map(str::to_owned)
            .or_else(|| std::env::var("SOLOIST_DISPLAY_VERSION").ok())
            .unwrap_or_else(|| "latest".into());
        let selected_channel = if let Ok(value) = std::env::var("SOLOIST_DISPLAY_CHANNEL") {
            Some(match value.as_str() {
                "stable" => ReleaseChannel::Stable,
                "alpha" => ReleaseChannel::Alpha,
                "beta" => ReleaseChannel::Beta,
                "prerelease" => ReleaseChannel::Prerelease,
                _ => bail!("SOLOIST_DISPLAY_CHANNEL must be stable, alpha, beta or prerelease"),
            })
        } else {
            channel
        };
        let base = if let Ok(custom) = std::env::var("SOLOIST_DISPLAY_RELEASE_BASE") {
            custom
        } else if requested == "latest" {
            let tag = self.find_release_tag(selected_channel)?;
            format!("https://github.com/jedrek0429/soloist-display/releases/download/{tag}")
        } else {
            let parsed = Version::parse(&requested)
                .context("SOLOIST_DISPLAY_VERSION must be canonical SemVer or latest")?;
            if parsed.to_string() != requested || !parsed.build.is_empty() {
                bail!("SOLOIST_DISPLAY_VERSION must be canonical SemVer");
            }
            format!("https://github.com/jedrek0429/soloist-display/releases/download/v{requested}")
        };
        let manifest_url = format!("{}/release-manifest.json", base.trim_end_matches('/'));
        let signature_url = format!("{}/release-manifest.sig", base.trim_end_matches('/'));
        let bytes = self
            .get_bytes(&manifest_url)
            .context("download release manifest")?;
        let signature = self
            .get_bytes(&signature_url)
            .context("download release signature")?;
        let verified = self.verify_manifest(bytes, signature)?;
        if requested != "latest" && verified.0.version.to_string() != requested {
            bail!(
                "signed release version {} does not match requested {requested}",
                verified.0.version
            );
        }
        if requested == "latest" {
            if let Some(channel) = selected_channel {
                if !channel.accepts(&verified.0.version) {
                    bail!("release {} does not belong to the {channel:?} channel", verified.0.version);
                }
            }
        }
        Ok(verified)
    }

    fn find_release_tag(&self, channel: Option<ReleaseChannel>) -> Result<String> {
        let url = "https://api.github.com/repos/jedrek0429/soloist-display/releases?per_page=100";
        let releases: Vec<serde_json::Value> = self
            .client
            .get(url)
            .send()?
            .error_for_status()?
            .json()
            .context("parse GitHub releases")?;
        select_release_tag(&releases, channel)
    }

    fn verify_manifest(
        &self,
        bytes: Vec<u8>,
        signature_bytes: Vec<u8>,
    ) -> Result<(ReleaseManifest, Vec<u8>)> {
        let encoded = String::from_utf8(signature_bytes)?.trim().to_owned();
        let signature_raw: [u8; 64] = STANDARD
            .decode(encoded)?
            .try_into()
            .map_err(|_| anyhow!("release signature must be 64 bytes"))?;
        self.public_key
            .verify(&bytes, &Signature::from_bytes(&signature_raw))
            .context("release manifest signature is invalid")?;
        let manifest: ReleaseManifest =
            serde_json::from_slice(&bytes).context("parse release manifest")?;
        if manifest.schema != 1 {
            bail!("unsupported release manifest schema {}", manifest.schema);
        }
        Ok((manifest, bytes))
    }

    fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let response = self.client.get(url).send()?.error_for_status()?;
        Ok(response.bytes()?.to_vec())
    }

    pub fn download(&self, artifact: &Artifact, destination: &Path) -> Result<()> {
        if artifact.size == 0 {
            bail!("artifact {} has invalid zero size", artifact.name);
        }
        let mut last_error = None;
        for url in std::iter::once(Some(artifact.url.as_str()))
            .chain(std::iter::once(artifact.fallback_url.as_deref()))
            .flatten()
        {
            for attempt in 1..=3 {
                match self.download_once(url, artifact, destination) {
                    Ok(()) => return Ok(()),
                    Err(error) => {
                        let _ = fs::remove_file(destination);
                        last_error = Some(error.context(format!(
                            "download {} from {url} (attempt {attempt}/3)",
                            artifact.name
                        )));
                        if attempt < 3 {
                            std::thread::sleep(Duration::from_millis(250 * attempt));
                        }
                    }
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("artifact {} has no download URL", artifact.name)))
    }

    fn download_once(&self, url: &str, artifact: &Artifact, destination: &Path) -> Result<()> {
        let mut response = self.client.get(url).send()?.error_for_status()?;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)?;
        let mut hasher = Sha256::new();
        let mut total = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = response.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > artifact.size {
                bail!("artifact {} exceeded declared size", artifact.name);
            }
            hasher.update(&buffer[..count]);
            file.write_all(&buffer[..count])?;
        }
        file.sync_all()?;
        if total != artifact.size {
            bail!(
                "artifact {} size mismatch: expected {}, received {total}",
                artifact.name,
                artifact.size
            );
        }
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(&artifact.sha256) {
            bail!("artifact {} SHA-256 mismatch", artifact.name);
        }
        Ok(())
    }
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::thread;

    fn test_client() -> ReleaseClient {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        ReleaseClient {
            client: Client::builder().build().unwrap(),
            public_key: key.verifying_key(),
        }
    }

    #[test]
    fn rejects_invalid_signature() {
        let client = ReleaseClient::new().unwrap();
        assert!(client.verify_manifest(br#"{"schema":1}"#.to_vec(), b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==\n".to_vec()).is_err());
    }

    #[test]
    fn embedded_release_key_verifies_rotation_fixture() {
        let client = ReleaseClient::new().unwrap();
        let signature: [u8; 64] = STANDARD
            .decode("HfVlf9MNfxCxtT2kKlyBVxjNh32JWWw9qaPkoWRjxoJLc0z1HZux0EmUqQvN0I22an47+f0JIajxPYGrxskjCg==")
            .unwrap()
            .try_into()
            .unwrap();
        client
            .public_key
            .verify(
                b"soloist-display release key test v1",
                &Signature::from_bytes(&signature),
            )
            .unwrap();
    }

    #[test]
    fn validates_signed_manifest_with_matching_key() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let client = test_client();
        let bytes = br#"{"schema":1,"version":"1.0.0","published_at":"2026-11-02T00:00:00Z","artifacts":[]}"#.to_vec();
        let signature = STANDARD.encode(key.sign(&bytes).to_bytes());
        let (manifest, _) = client
            .verify_manifest(bytes, signature.into_bytes())
            .unwrap();
        assert_eq!(manifest.version, Version::new(1, 0, 0));
    }

    #[test]
    fn corrupt_primary_is_removed_before_verified_fallback() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = format!("http://{}", server.server_addr());
        let worker = thread::spawn(move || {
            for _ in 0..3 {
                server
                    .recv()
                    .unwrap()
                    .respond(tiny_http::Response::from_data(b"baad".to_vec()))
                    .unwrap();
            }
            server
                .recv()
                .unwrap()
                .respond(tiny_http::Response::from_data(b"good".to_vec()))
                .unwrap();
        });
        let artifact = Artifact {
            name: "fixture".into(),
            kind: "manager".into(),
            target: "test".into(),
            url: format!("{address}/primary"),
            fallback_url: Some(format!("{address}/fallback")),
            sha256: hex::encode(Sha256::digest(b"good")),
            size: 4,
        };
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("artifact");
        test_client().download(&artifact, &destination).unwrap();
        assert_eq!(fs::read(destination).unwrap(), b"good");
        worker.join().unwrap();
    }

    #[test]
    fn interrupted_download_and_missing_fallback_fail_closed() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = format!("http://{}", server.server_addr());
        let worker = thread::spawn(move || {
            for _ in 0..3 {
                server
                    .recv()
                    .unwrap()
                    .respond(tiny_http::Response::from_data(b"short".to_vec()))
                    .unwrap();
            }
            for _ in 0..3 {
                server
                    .recv()
                    .unwrap()
                    .respond(tiny_http::Response::empty(503))
                    .unwrap();
            }
        });
        let artifact = Artifact {
            name: "fixture".into(),
            kind: "manager".into(),
            target: "test".into(),
            url: format!("{address}/primary"),
            fallback_url: Some(format!("{address}/fallback")),
            sha256: "0".repeat(64),
            size: 100,
        };
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("artifact");
        assert!(test_client().download(&artifact, &destination).is_err());
        assert!(!destination.exists());
        worker.join().unwrap();
    }
}

#[cfg(test)]
mod channel_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stable_is_default_after_first_stable_release() {
        let releases = vec![
            json!({"draft":false,"tag_name":"v1.2.0-beta.3"}),
            json!({"draft":false,"tag_name":"v1.0.0"}),
            json!({"draft":false,"tag_name":"v1.1.0-beta.4"}),
            json!({"draft":false,"tag_name":"v2.0.0-rc.1"}),
            json!({"draft":true,"tag_name":"v9.0.0"}),
        ];
        assert_eq!(select_release_tag(&releases, None).unwrap(), "v1.0.0");
        assert_eq!(
            select_release_tag(&releases, Some(ReleaseChannel::Stable)).unwrap(),
            "v1.0.0"
        );
        assert_eq!(
            select_release_tag(&releases, Some(ReleaseChannel::Prerelease)).unwrap(),
            "v1.2.0-beta.3"
        );
    }

    #[test]
    fn only_alpha_and_beta_are_considered_for_latest() {
        let releases = vec![
            json!({"draft":false,"tag_name":"v2.0.0-rc.1"}),
            json!({"draft":false,"tag_name":"v1.1.0-alpha.8"}),
            json!({"draft":false,"tag_name":"v1.1.0-beta.67"}),
            json!({"draft":false,"tag_name":"v1.0.0"}),
        ];
        assert_eq!(select_release_tag(&releases, None).unwrap(), "v1.0.0");
        assert_eq!(
            select_release_tag(&releases, Some(ReleaseChannel::Prerelease)).unwrap(),
            "v1.1.0-beta.67"
        );
    }

    #[test]
    fn alpha_and_beta_are_separate_update_tracks() {
        let releases = vec![
            json!({"draft":false,"tag_name":"v1.1.0-alpha.9"}),
            json!({"draft":false,"tag_name":"v1.1.0-beta.67"}),
            json!({"draft":false,"tag_name":"v1.2.0-alpha.1"}),
            json!({"draft":false,"tag_name":"v1.0.0"}),
            json!({"draft":false,"tag_name":"v2.0.0-rc.1"}),
        ];
        assert_eq!(select_release_tag(&releases, None).unwrap(), "v1.0.0");
        assert_eq!(
            select_release_tag(&releases, Some(ReleaseChannel::Alpha)).unwrap(),
            "v1.2.0-alpha.1"
        );
        assert_eq!(
            select_release_tag(&releases, Some(ReleaseChannel::Beta)).unwrap(),
            "v1.1.0-beta.67"
        );
        assert_eq!(
            select_release_tag(&releases, Some(ReleaseChannel::Prerelease)).unwrap(),
            "v1.2.0-alpha.1"
        );
    }

    #[test]
    fn missing_channel_does_not_fall_back_to_another_channel() {
        let releases = vec![json!({"draft":false,"tag_name":"v1.1.0-alpha.1"})];
        assert!(select_release_tag(&releases, Some(ReleaseChannel::Beta)).is_err());
        assert!(select_release_tag(&releases, Some(ReleaseChannel::Stable)).is_err());
    }

    #[test]
    fn installed_version_determines_its_specific_channel() {
        assert_eq!(
            ReleaseChannel::for_version(&Version::parse("1.0.0").unwrap()),
            ReleaseChannel::Stable
        );
        assert_eq!(
            ReleaseChannel::for_version(&Version::parse("1.1.0-alpha.7").unwrap()),
            ReleaseChannel::Alpha
        );
        assert_eq!(
            ReleaseChannel::for_version(&Version::parse("1.1.0-beta.67").unwrap()),
            ReleaseChannel::Beta
        );
    }

    #[test]
    fn first_public_release_can_be_alpha() {
        let releases = vec![json!({"draft":false,"tag_name":"v1.0.0-alpha.1"})];
        assert_eq!(
            select_release_tag(&releases, None).unwrap(),
            "v1.0.0-alpha.1"
        );
        assert!(select_release_tag(&releases, Some(ReleaseChannel::Stable)).is_err());
    }
}
