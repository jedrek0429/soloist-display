use crate::platform::Platform;
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
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
pub const GITHUB_LATEST: &str =
    "https://github.com/jedrek0429/soloist-display/releases/latest/download";

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

    pub fn soloist<'a>(&'a self, platform: &Platform) -> Result<&'a Artifact> {
        let archive = platform.soloist_archive();
        self.artifacts
            .iter()
            .find(|artifact| artifact.kind == "soloist" && artifact.target == archive)
            .ok_or_else(|| {
                anyhow!(
                    "release {} has no verified Spotify Soloist artifact for {archive}",
                    self.version
                )
            })
    }
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
        let base = if let Ok(custom) = std::env::var("SOLOIST_DISPLAY_RELEASE_BASE") {
            custom
        } else {
            let requested =
                std::env::var("SOLOIST_DISPLAY_VERSION").unwrap_or_else(|_| "latest".into());
            if requested == "latest" {
                if std::env::var("SOLOIST_DISPLAY_CHANNEL").as_deref() == Ok("prerelease")
                    || (std::env::var_os("SOLOIST_DISPLAY_CHANNEL").is_none()
                        && !Version::parse(env!("CARGO_PKG_VERSION"))?.pre.is_empty())
                {
                    self.latest_prerelease_base()?
                } else {
                    GITHUB_LATEST.into()
                }
            } else {
                let parsed = Version::parse(&requested)
                    .context("SOLOIST_DISPLAY_VERSION must be X.Y.Z or latest")?;
                if parsed.to_string() != requested {
                    bail!("SOLOIST_DISPLAY_VERSION must be canonical X.Y.Z");
                }
                format!(
                    "https://github.com/jedrek0429/soloist-display/releases/download/v{requested}"
                )
            }
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
        if let Ok(requested) = std::env::var("SOLOIST_DISPLAY_VERSION") {
            if requested != "latest" && verified.0.version.to_string() != requested {
                bail!(
                    "signed release version {} does not match requested {requested}",
                    verified.0.version
                );
            }
        }
        Ok(verified)
    }

    // GitHub /releases/latest excludes prereleases. Explicit prerelease channel
    // discovers the highest published SemVer; a stable 1.0.0 supersedes its RC.
    fn latest_prerelease_base(&self) -> Result<String> {
        let url = "https://api.github.com/repos/jedrek0429/soloist-display/releases?per_page=100";
        let releases: Vec<serde_json::Value> = self.client.get(url).send()?
            .error_for_status()?.json().context("parse GitHub releases")?;
        let selected = releases.into_iter().filter_map(|release| {
            if release["draft"].as_bool() != Some(false) || release["prerelease"].as_bool().is_none() {
                return None;
            }
            let tag = release["tag_name"].as_str()?;
            let version = Version::parse(tag.strip_prefix('v')?).ok()?;
            Some((version, tag.to_owned()))
        }).max_by(|left, right| left.0.cmp(&right.0))
          .ok_or_else(|| anyhow!("no published prerelease or stable release available"))?;
        Ok(format!(
            "https://github.com/jedrek0429/soloist-display/releases/download/{}",
            selected.1
        ))
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
