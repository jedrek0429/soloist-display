#!/usr/bin/env python3
"""Render a release bootstrap with immutable installer URLs and digests."""
import pathlib
import sys

if len(sys.argv) != 4:
    raise SystemExit("usage: render-bootstrap.py TEMPLATE BOOTSTRAP_MANIFEST RELEASE_BASE")
template = pathlib.Path(sys.argv[1]).read_text()
records = {}
for line in pathlib.Path(sys.argv[2]).read_text().splitlines():
    target, digest, _url = line.split()
    records[target] = digest
replacements = {
    "@SOLOIST_DISPLAY_RELEASE_BASE@": sys.argv[3],
    "@SOLOIST_DISPLAY_SHA_X86_64@": records["x86_64-unknown-linux-musl"],
    "@SOLOIST_DISPLAY_SHA_AARCH64@": records["aarch64-unknown-linux-musl"],
    "@SOLOIST_DISPLAY_SHA_ARMV7@": records["armv7-unknown-linux-musleabihf"],
}
for old, new in replacements.items():
    template = template.replace(old, new)
if "@SOLOIST_DISPLAY_" in template:
    raise SystemExit("unresolved bootstrap placeholder")
sys.stdout.write(template)
