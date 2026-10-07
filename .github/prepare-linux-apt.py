#!/usr/bin/env python3
"""Prepare bounded HTTPS APT acquisition on disposable hosted Linux runners."""
import os
import platform
from pathlib import Path


def prepare(apt_root: Path, machine: str = "x86_64") -> None:
    if machine.lower() not in ("x86_64", "amd64", "aarch64", "arm64"):
        raise ValueError("unsupported hosted Linux architecture")
    arm = machine.lower() in ("aarch64", "arm64")
    archive = "https://ports.ubuntu.com/ubuntu-ports" if arm else "https://archive.ubuntu.com/ubuntu"
    security = archive if arm else "https://security.ubuntu.com/ubuntu"
    # Preserve mirror+file paths: existing cached package lists still use them.
    for name, uri in (
        ("apt-mirrors.txt", archive),
        ("apt-mirrors-security.txt", security),
    ):
        mirror = apt_root / name
        if mirror.is_file():
            mirror.write_text(uri + "\n")
    sources = [apt_root / "sources.list"]
    sources.extend((apt_root / "sources.list.d").glob("*.list"))
    sources.extend((apt_root / "sources.list.d").glob("*.sources"))
    for source in sources:
        if not source.is_file():
            continue
        before = source.read_text()
        after = before
        for old, new in (
            ("http://azure.archive.ubuntu.com/ubuntu", archive),
            ("http://archive.ubuntu.com/ubuntu", archive),
            ("http://security.ubuntu.com/ubuntu", security),
            ("http://ports.ubuntu.com/ubuntu-ports", "https://ports.ubuntu.com/ubuntu-ports"),
        ):
            after = after.replace(old, new)
        if after != before:
            source.write_text(after)
    config = apt_root / "apt.conf.d" / "99-vesper-ci-acquire"
    config.write_text('Acquire::ForceIPv4 "true";\nAcquire::Retries "2";\n'
                      'Acquire::http::Timeout "10";\nAcquire::https::Timeout "10";\n')


if __name__ == "__main__":
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_OS") != "Linux":
        raise SystemExit("APT preparation requires a GitHub-hosted Linux runner")
    prepare(Path("/etc/apt"), platform.machine())
