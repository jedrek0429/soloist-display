//! Shared release acquisition for externally distributed add-on executables.
//! Add-on definitions select the upstream release and asset; the manager owns
//! transport, checksum validation, archive extraction and ELF verification.
use crate::platform::Architecture;
use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path, time::Duration};
use tar::Archive;

pub struct ReleaseSpec {
    pub repository: &'static str,
    pub version: &'static str,
    pub binary: &'static str,
    pub asset: fn(&Architecture) -> Result<&'static str>,
    pub max_archive_bytes: u64,
    pub max_binary_bytes: u64,
}

fn get(client: &Client, url: &str, maximum: u64) -> Result<Vec<u8>> {
    let response = client.get(url).send()
        .with_context(|| format!("download {url}"))?
        .error_for_status().with_context(|| format!("download {url}"))?;
    let mut body = Vec::new();
    response.take(maximum + 1).read_to_end(&mut body)?;
    if body.len() as u64 > maximum {
        bail!("add-on release asset exceeds its size limit");
    }
    Ok(body)
}

fn verify_checksum(archive: &[u8], content: &str, name: &str) -> Result<()> {
    let mut fields = content.split_whitespace();
    let digest = fields.next().context("missing add-on checksum")?;
    let filename = fields.next().context("missing add-on checksum filename")?;
    if fields.next().is_some()
        || digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        || filename.trim_start_matches('*') != name
    {
        bail!("invalid upstream add-on checksum file");
    }
    let actual = hex::encode(Sha256::digest(archive));
    if !actual.eq_ignore_ascii_case(digest) {
        bail!("add-on release checksum mismatch");
    }
    Ok(())
}

fn verify_binary(binary: &[u8], architecture: &Architecture) -> Result<()> {
    if binary.len() < 40 || &binary[..4] != b"\x7fELF" || binary[5] != 1 {
        bail!("upstream add-on executable is not a little-endian ELF");
    }
    let machine = u16::from_le_bytes([binary[18], binary[19]]);
    let expected = match architecture {
        Architecture::X86_64 => (2, 62),
        Architecture::Aarch64 => (2, 183),
        Architecture::Armv7 => (1, 40),
    };
    if (binary[4], machine) != expected {
        bail!("upstream add-on executable has the wrong architecture");
    }
    Ok(())
}

fn unpack_binary(archive: &[u8], spec: &ReleaseSpec, architecture: &Architecture) -> Result<Vec<u8>> {
    let decoder = GzDecoder::new(archive);
    let mut tar = Archive::new(decoder);
    let mut binary = None;
    for entry in tar.entries().context("read upstream add-on archive")? {
        let mut entry = entry?;
        if entry.path()?.as_ref() != Path::new(spec.binary) {
            continue;
        }
        if !entry.header().entry_type().is_file() || binary.is_some() {
            bail!("invalid or duplicate add-on executable in archive");
        }
        if entry.size() > spec.max_binary_bytes {
            bail!("add-on executable exceeds its size limit");
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        verify_binary(&bytes, architecture)?;
        binary = Some(bytes);
    }
    binary.with_context(|| format!("archive does not contain {}", spec.binary))
}

/// Download a specific trusted release; this deliberately does not execute it.
pub fn download_binary(spec: &ReleaseSpec, architecture: &Architecture) -> Result<Vec<u8>> {
    let asset = (spec.asset)(architecture)?;
    let root = format!(
        "https://github.com/{}/releases/download/v{}",
        spec.repository, spec.version
    );
    let client = Client::builder().timeout(Duration::from_secs(40)).build()?;
    let archive = get(&client, &format!("{root}/{asset}"), spec.max_archive_bytes)?;
    let checksum = get(&client, &format!("{root}/{asset}.sha256"), 4096)?;
    verify_checksum(
        &archive,
        std::str::from_utf8(&checksum).context("invalid checksum encoding")?,
        asset,
    )?;
    unpack_binary(&archive, spec, architecture)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_is_exact_and_filename_bound() {
        let archive = b"archive";
        let digest = hex::encode(Sha256::digest(archive));
        assert!(verify_checksum(archive, &format!("{digest}  bridge.tar.gz"), "bridge.tar.gz").is_ok());
        assert!(verify_checksum(archive, &format!("{digest}  other.tar.gz"), "bridge.tar.gz").is_err());
        assert!(verify_checksum(b"wrong", &format!("{digest}  bridge.tar.gz"), "bridge.tar.gz").is_err());
    }

    #[test]
    fn binary_rejects_wrong_architecture() {
        let mut binary = vec![0; 40];
        binary[..4].copy_from_slice(b"\x7fELF");
        binary[4] = 2;
        binary[5] = 1;
        binary[18..20].copy_from_slice(&62u16.to_le_bytes());
        assert!(verify_binary(&binary, &Architecture::X86_64).is_ok());
        assert!(verify_binary(&binary, &Architecture::Aarch64).is_err());
    }
}
