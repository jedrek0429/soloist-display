#!/usr/bin/env python3
"""Create deterministic signed-release inputs from validated artifacts."""
import hashlib
import json
import os
import pathlib
import sys
from datetime import datetime, timezone

if len(sys.argv) != 4:
    raise SystemExit("usage: build-release-manifest.py DIST TAG VERSION")
dist = pathlib.Path(sys.argv[1])
tag, version = sys.argv[2:]
repo = os.environ.get("GITHUB_REPOSITORY", "jedrek0429/soloist-display")
release = f"https://github.com/{repo}/releases/download/{tag}"
latest = f"https://github.com/{repo}/releases/latest/download"

executables = {
    "x86_64-unknown-linux-musl": ("soloist-display-x86_64-unknown-linux-musl", "soloist-audio-bridge-x86_64-unknown-linux-musl"),
    "aarch64-unknown-linux-musl": ("soloist-display-aarch64-unknown-linux-musl", "soloist-audio-bridge-aarch64-unknown-linux-musl"),
    "armv7-unknown-linux-musleabihf": ("soloist-display-armv7-unknown-linux-musleabihf", "soloist-audio-bridge-armv7-unknown-linux-musleabihf"),
    "x86_64-pc-windows-msvc": ("soloist-display-x86_64-pc-windows-msvc.exe", "soloist-audio-bridge-x86_64-pc-windows-msvc.exe"),
    "aarch64-pc-windows-msvc": ("soloist-display-aarch64-pc-windows-msvc.exe", "soloist-audio-bridge-aarch64-pc-windows-msvc.exe"),
    "x86_64-apple-darwin": ("soloist-display-x86_64-apple-darwin", "soloist-audio-bridge-x86_64-apple-darwin"),
    "aarch64-apple-darwin": ("soloist-display-aarch64-apple-darwin", "soloist-audio-bridge-aarch64-apple-darwin"),
}

soloist_archives = {
    "linux-x86_64": "soloist_release_x86_64.tar.gz",
    "linux-arm64": "soloist_release_arm64.tar.gz",
    "linux-arm32": "soloist_release_arm32.tar.gz",
    "windows-x86_64": "soloist_release_windows_x86_64.zip",
    "macos-arm64": "soloist_release_osx_arm64.tar.gz",
}

def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()

artifacts, bootstrap = [], []
for target, names in executables.items():
    paths = [dist / name for name in names]
    if not any(path.exists() for path in paths):
        continue
    if not all(path.is_file() for path in paths):
        raise SystemExit(f"incomplete executable pair for {target}")
    for kind, name, path in zip(("manager", "audio-bridge"), names, paths):
        sha = digest(path)
        artifacts.append({"name": name, "kind": kind, "target": target, "url": f"{release}/{name}",
                          "fallback_url": f"{latest}/{name}", "sha256": sha, "size": path.stat().st_size})
        if kind == "manager":
            bootstrap.append(f"{target} {sha} {release}/{name}\n")

for target, name in soloist_archives.items():
    path = dist / name
    if path.is_file():
        artifacts.append({"name": name, "kind": "soloist", "target": target,
                          "url": f"https://soloist-builds.spotifycdn.com/{name}",
                          "sha256": digest(path), "size": path.stat().st_size})

manifest = {"schema": 1, "version": version,
            "published_at": datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
            "artifacts": artifacts}
(dist / "release-manifest.json").write_text(json.dumps(manifest, separators=(",", ":")) + "\n")
(dist / "bootstrap-manifest.txt").write_text("".join(bootstrap))
