#!/usr/bin/env python3
"""Select the previous published release for generated GitHub release notes.

Input: current tag as argv[1], published tags (one per line) on stdin.
Output: exactly one previous tag, or nothing for the first release.
"""
import re
import sys

PATTERN = re.compile(r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-(alpha|beta)\.([1-9][0-9]*))?$")


def parse(tag):
    match = PATTERN.fullmatch(tag)
    if not match:
        return None
    major, minor, patch, channel, number = match.groups()
    return (int(major), int(minor), int(patch)), channel or "stable", int(number or 0)


def pick(current_tag, published_tags):
    current = parse(current_tag)
    if current is None:
        raise ValueError(f"Unsupported release tag: {current_tag}")
    version, channel, sequence = current

    candidates = []
    for tag in published_tags:
        tag = tag.strip()
        candidate = parse(tag)
        if candidate is None or tag == current_tag:
            continue
        v, c, n = candidate
        if channel == "stable":
            if c == "stable" and v < version:
                candidates.append((v, 0, tag))
        elif v == version:
            if c == channel and n < sequence:
                candidates.append(((n,), 2, tag))
            elif channel == "beta" and c == "alpha":
                candidates.append(((n,), 1, tag))
        elif c == "stable" and v < version:
            # Only the first prerelease in a series falls back to stable.
            candidates.append((v, 0, tag))

    if channel != "stable":
        same_version = [(key, rank, tag) for key, rank, tag in candidates if rank > 0]
        if same_version:
            # Current-channel releases win, then previous-channel releases.
            return max(same_version, key=lambda item: (item[1], item[0]))[2]
    return max(candidates, key=lambda item: item[0])[2] if candidates else ""


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("Usage: release-notes-base.py vX.Y.Z[-alpha.N|-beta.N]")
    print(pick(sys.argv[1], sys.stdin))
