#!/usr/bin/env python3
"""Regression tests for release manifests and rendered bootstraps."""

import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[1]


def run(*arguments: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, *arguments],
        cwd=ROOT,
        check=check,
        text=True,
        capture_output=True,
    )


def write(path: pathlib.Path, content: bytes) -> str:
    path.write_bytes(content)
    return hashlib.sha256(content).hexdigest()


def test_manifest() -> None:
    with tempfile.TemporaryDirectory() as temporary:
        dist = pathlib.Path(temporary)
        linux_manager = write(dist / "soloist-display-x86_64-unknown-linux-musl", b"manager")
        write(dist / "soloist-audio-bridge-x86_64-unknown-linux-musl", b"bridge")
        windows_manager = write(dist / "soloist-display-x86_64-pc-windows-msvc.exe", b"manager.exe")
        write(dist / "soloist-audio-bridge-x86_64-pc-windows-msvc.exe", b"bridge.exe")
        write(dist / "soloist_release_x86_64.tar.gz", b"upstream")

        run("scripts/build-release-manifest.py", str(dist), "v1.2.3", "1.2.3")
        manifest = json.loads((dist / "release-manifest.json").read_text())
        records = {(item["kind"], item["target"]): item for item in manifest["artifacts"]}
        assert records[("manager", "x86_64-unknown-linux-musl")]["sha256"] == linux_manager
        assert records[("manager", "x86_64-pc-windows-msvc")]["sha256"] == windows_manager
        assert records[("soloist", "linux-x86_64")]["url"].endswith(
            "/soloist_release_x86_64.tar.gz"
        )
        assert len((dist / "bootstrap-manifest.txt").read_text().splitlines()) == 2

        (dist / "soloist-audio-bridge-x86_64-pc-windows-msvc.exe").unlink()
        failed = run(
            "scripts/build-release-manifest.py", str(dist), "v1.2.3", "1.2.3", check=False
        )
        assert failed.returncode != 0
        assert "incomplete executable pair" in failed.stderr


def test_rendering() -> None:
    with tempfile.TemporaryDirectory() as temporary:
        directory = pathlib.Path(temporary)
        manifest = directory / "bootstrap.txt"
        manifest.write_text(
            "x86_64-unknown-linux-musl " + "a" * 64 + " https://example/linux\n"
            "x86_64-pc-windows-msvc " + "b" * 64 + " https://example/windows\n"
        )
        shell = run(
            "scripts/render-bootstrap.py",
            "install.sh",
            str(manifest),
            "https://example/v1.2.3",
        ).stdout
        powershell = run(
            "scripts/render-bootstrap.py",
            "install.ps1",
            str(manifest),
            "https://example/v1.2.3",
        ).stdout
        assert "PIN_X86_64='" + "a" * 64 + "'" in shell
        assert "PIN_MACOS_ARM64='unavailable'" in shell
        assert "$pinnedX64 = '" + "b" * 64 + "'" in powershell
        assert "$pinnedArm64 = 'unavailable'" in powershell


if __name__ == "__main__":
    test_manifest()
    test_rendering()
    print("release tool tests passed")
