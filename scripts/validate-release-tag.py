#!/usr/bin/env python3
"""Validate canonical SemVer tags; version equality is checked by release.yml."""
import re
import sys

TAG = re.compile(
    r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
)
def valid(tag):
    match = TAG.fullmatch(tag)
    if not match:
        return False
    prerelease = match.group(4)
    return not prerelease or all(
        not (part.isascii() and part.isdigit() and len(part) > 1 and part[0] == "0")
        for part in prerelease.split(".")
    )

if __name__ == "__main__":
    if len(sys.argv) != 2 or not valid(sys.argv[1]):
        raise SystemExit("Invalid release tag: expected canonical vMAJOR.MINOR.PATCH[-PRERELEASE]")
