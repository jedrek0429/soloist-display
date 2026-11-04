use anyhow::{anyhow, bail, Context, Result};
use semver::Version;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Read,
    path::Path,
    process::Command,
};

const TARGETS: [&str; 3] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "armv7-unknown-linux-gnueabihf",
];
const LEGACY_TARGETS: [&str; 3] = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "armv7-unknown-linux-musleabihf",
];
const STYLES: [&str; 6] = [
    "Basic",
    "Cd",
    "Vinyl",
    "VinylCover",
    "CdCover",
    "AlbumCover",
];

fn valid_tag(tag: &str) -> Result<Version> {
    let version = tag
        .strip_prefix('v')
        .ok_or_else(|| anyhow!("tag must begin with v"))?;
    let parsed = Version::parse(version).context("invalid SemVer release tag")?;
    if format!("v{parsed}") != tag {
        bail!("tag is not canonical SemVer");
    }
    Ok(parsed)
}

fn digest(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("read {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn timestamp() -> Result<String> {
    let output = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()?;
    if !output.status.success() {
        bail!("date -u failed");
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn build_manifest(dist: &Path, tag: &str, version: &str) -> Result<()> {
    let parsed = valid_tag(tag)?;
    if parsed.to_string() != version {
        bail!("tag and release version mismatch");
    }
    let repo =
        env::var("GITHUB_REPOSITORY").unwrap_or_else(|_| "jedrek0429/soloist-display".into());
    let base = format!("https://github.com/{repo}/releases/download/{tag}");
    let mut artifacts = Vec::new();
    let mut bootstrap = String::new();
    for (index, target) in TARGETS.iter().enumerate() {
        for (kind, prefix) in [
            ("manager", "soloist-display"),
            ("audio-bridge", "soloist-audio-bridge"),
        ] {
            let name = format!("{prefix}-{target}");
            let file = dist.join(&name);
            let sha = digest(&file)?;
            let size = file.metadata()?.len();
            for selected_target in [*target, LEGACY_TARGETS[index]] {
                artifacts.push(json!({
                    "name": name, "kind": kind, "target": selected_target,
                    "url": format!("{base}/{name}"), "fallback_url": null,
                    "sha256": sha, "size": size
                }));
                if kind == "manager" {
                    bootstrap.push_str(&format!("{selected_target} {sha} {base}/{name}\n"));
                }
            }
        }
    }
    let manifest = json!({
        "schema": 1, "version": version, "published_at": timestamp()?,
        "artifacts": artifacts
    });
    fs::write(
        dist.join("release-manifest.json"),
        format!("{}\n", serde_json::to_string(&manifest)?),
    )?;
    fs::write(dist.join("bootstrap-manifest.txt"), bootstrap)?;
    Ok(())
}

fn render_bootstrap(template: &Path, manifest: &Path, base: &str) -> Result<()> {
    if !base.starts_with("https://github.com/") || !base.contains("/releases/download/v") {
        bail!("invalid immutable release base");
    }
    let contents = fs::read_to_string(manifest)?;
    let mut digests = BTreeMap::new();
    for line in contents.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 3 {
            bail!("invalid bootstrap manifest record");
        }
        let (target, sha, url) = (fields[0], fields[1], fields[2]);
        if (!TARGETS.contains(&target) && !LEGACY_TARGETS.contains(&target))
            || sha.len() != 64
            || !sha.bytes().all(|b| b.is_ascii_hexdigit())
            || !url.starts_with(&format!("{base}/soloist-display-{target}"))
        {
            bail!("invalid bootstrap record for {target}");
        }
        if digests.insert(target, sha).is_some() {
            bail!("duplicate bootstrap target: {target}");
        }
    }
    if digests.len() != TARGETS.len() + LEGACY_TARGETS.len() {
        bail!("bootstrap manifest has missing targets");
    }
    let mut rendered = fs::read_to_string(template)?;
    for (placeholder, value) in [
        ("@SOLOIST_DISPLAY_RELEASE_BASE@", base),
        ("@SOLOIST_DISPLAY_SHA_X86_64@", digests[TARGETS[0]]),
        ("@SOLOIST_DISPLAY_SHA_AARCH64@", digests[TARGETS[1]]),
        ("@SOLOIST_DISPLAY_SHA_ARMV7@", digests[TARGETS[2]]),
    ] {
        if !rendered.contains(placeholder) {
            bail!("missing template token: {placeholder}");
        }
        rendered = rendered.replace(placeholder, value);
    }
    if rendered.contains("@SOLOIST_DISPLAY_") {
        bail!("unresolved bootstrap placeholder");
    }
    print!("{rendered}");
    Ok(())
}

fn check_settings(root: &Path) -> Result<()> {
    let config: Map<String, Value> =
        serde_json::from_str(&fs::read_to_string(root.join("config.json"))?)?;
    let source = fs::read_to_string(root.join("src/settings.js"))?;
    let virtual_keys: BTreeSet<_> = ["discOpacity", "discSize", "centerHole", "centerHoleColor"]
        .into_iter()
        .collect();
    let presets: BTreeSet<_> = [
        "transitionStyle",
        "transitionDuration",
        "transitionDelay",
        "sleeveTransitionStyle",
        "sleeveAlignment",
        "sleeveDuration",
        "sleeveDelayIn",
        "sleeveDelayOut",
    ]
    .into_iter()
    .collect();
    let mut missing = BTreeSet::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("['") {
            continue;
        }
        let mut parts = trimmed.split('\'');
        let _ = parts.next();
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(_separator) = parts.next() else {
            continue;
        };
        let Some(_label) = parts.next() else {
            continue;
        };
        let Some(_separator) = parts.next() else {
            continue;
        };
        let Some(kind) = parts.next() else {
            continue;
        };
        if !["select", "range", "toggle", "text"].contains(&kind) {
            continue;
        }
        if !virtual_keys.contains(key) && !presets.contains(key) && !config.contains_key(key) {
            missing.insert(key.to_owned());
        }
    }
    for style in STYLES {
        for key in ["transitionStyle", "transitionDuration", "transitionDelay"] {
            let name = format!("{key}{style}");
            if !config.contains_key(&name) {
                missing.insert(name);
            }
        }
    }
    for style in ["VinylCover", "CdCover"] {
        for key in [
            "sleeveTransitionStyle",
            "sleeveAlignment",
            "sleeveDuration",
            "sleeveDelayIn",
            "sleeveDelayOut",
        ] {
            let name = format!("{key}{style}");
            if !config.contains_key(&name) {
                missing.insert(name);
            }
        }
    }
    for key in [
        "discSizeBasic",
        "discSizeCd",
        "discSizeVinyl",
        "discSizeVinylCover",
        "discSizeCdCover",
        "discSizeAlbumCover",
        "discOpacityBasic",
        "discOpacityCd",
        "discOpacityVinyl",
        "discOpacityVinylCover",
        "discOpacityCdCoverUp",
        "discOpacityCdCoverDown",
        "discOpacityAlbumCover",
        "centerHoleBasic",
        "centerHoleCd",
        "centerHoleVinyl",
        "centerHoleColorBasic",
        "centerHoleColorCd",
        "centerHoleColorVinyl",
    ] {
        if !config.contains_key(key) {
            missing.insert(key.to_owned());
        }
    }
    if !missing.is_empty() {
        bail!(
            "Settings UI keys missing from config.json: {}",
            missing.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    println!("Settings UI keys match config schema");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, tag] if cmd == "validate-tag" => {
            valid_tag(tag)?;
            println!("{tag}");
        },
        [cmd, old, new] if cmd == "validate-bump" => {
            let current = Version::parse(old).context("invalid current version")?;
            let requested = Version::parse(new).context("invalid new version")?;
            if requested <= current {
                bail!("version {requested} must be newer than {current}");
            }
        },
        [cmd, dist, tag, version] if cmd == "build-manifest" =>
            build_manifest(Path::new(dist), tag, version)?,
        [cmd, template, manifest, base] if cmd == "render-bootstrap" =>
            render_bootstrap(Path::new(template), Path::new(manifest), base)?,
        [cmd, root] if cmd == "check-settings" =>
            check_settings(Path::new(root))?,
        [cmd, tag] if cmd == "verify-workspace-version" => {
            let tag_version = valid_tag(tag)?;
            let actual = Version::parse(env!("CARGO_PKG_VERSION"))?;
            if tag_version != actual {
                bail!("tag {tag} does not match workspace version {actual}");
            }
        },
        _ => bail!("usage: release-tools validate-tag TAG | validate-bump OLD NEW | verify-workspace-version TAG | build-manifest DIST TAG VERSION | render-bootstrap TEMPLATE MANIFEST BASE | check-settings ROOT"),
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_does_not_pin_rolling_spotify_archives() {
        let dir = tempfile::tempdir().unwrap();
        for target in TARGETS {
            for prefix in ["soloist-display", "soloist-audio-bridge"] {
                fs::write(dir.path().join(format!("{prefix}-{target}")), b"test").unwrap();
            }
        }
        build_manifest(dir.path(), "v1.0.0-alpha.4", "1.0.0-alpha.4").unwrap();
        let parsed: Value =
            serde_json::from_slice(&fs::read(dir.path().join("release-manifest.json")).unwrap())
                .unwrap();
        let artifacts = parsed["artifacts"].as_array().unwrap();
        assert_eq!(artifacts.len(), 12);
        for (current, legacy) in TARGETS.into_iter().zip(LEGACY_TARGETS) {
            let new_asset = artifacts
                .iter()
                .find(|asset| asset["kind"] == "manager" && asset["target"] == current)
                .unwrap();
            let old_asset = artifacts
                .iter()
                .find(|asset| asset["kind"] == "manager" && asset["target"] == legacy)
                .unwrap();
            assert_eq!(new_asset["url"], old_asset["url"]);
            assert_eq!(new_asset["sha256"], old_asset["sha256"]);
        }
        assert!(artifacts
            .iter()
            .all(|artifact| artifact["kind"] != "soloist"));
    }

    #[test]
    fn accepts_canonical_release_tags() {
        for tag in ["v1.0.0", "v1.0.0-alpha.1", "v1.0.0-beta.2", "v1.0.0-rc.1"] {
            assert!(valid_tag(tag).is_ok(), "{tag}");
        }
    }
    #[test]
    fn compares_prerelease_versions() {
        let stable = Version::parse("1.0.0").unwrap();
        let alpha = Version::parse("1.0.0-alpha.2").unwrap();
        let beta = Version::parse("1.0.0-beta.1").unwrap();
        let rc = Version::parse("1.0.0-rc.1").unwrap();
        assert!(alpha < beta);
        assert!(beta < rc);
        assert!(rc < stable);
    }
    #[test]
    fn rejects_invalid_tags() {
        for tag in [
            "1.0.0",
            "v01.0.0",
            "v1.0",
            "v1.0.0-alpha.01",
            "v1.0.0-foo/bar",
            "v1.0.0-",
        ] {
            assert!(valid_tag(tag).is_err(), "{tag}");
        }
    }
}
