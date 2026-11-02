#!/usr/bin/env python3
"""Create deterministic signed-release inputs from an already-built dist directory."""
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
targets = ["x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl", "armv7-unknown-linux-musleabihf"]

def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()

artifacts, bootstrap = [], []
for target in targets:
    for kind, prefix in (("manager", "soloist-display"), ("audio-bridge", "soloist-audio-bridge")):
        name, path = f"{prefix}-{target}", dist / f"{prefix}-{target}"
        sha = digest(path)
        artifacts.append({"name": name, "kind": kind, "target": target, "url": f"{release}/{name}",
                          "fallback_url": f"{latest}/{name}", "sha256": sha, "size": path.stat().st_size})
        if kind == "manager":
            bootstrap.append(f"{target} {sha} {release}/{name}\n")
for architecture in ("x86_64", "arm64", "arm32"):
    name, path = f"soloist_release_{architecture}.tar.gz", dist / f"soloist_release_{architecture}.tar.gz"
    artifacts.append({"name": name, "kind": "soloist", "target": architecture,
                      "url": f"https://soloist-builds.spotifycdn.com/{name}",
                      "sha256": digest(path), "size": path.stat().st_size})
manifest = {"schema": 1, "version": version,
            "published_at": datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z"),
            "artifacts": artifacts}
(dist / "release-manifest.json").write_text(json.dumps(manifest, separators=(",", ":")) + "\n")
(dist / "bootstrap-manifest.txt").write_text("".join(bootstrap))
