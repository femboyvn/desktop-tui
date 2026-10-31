#!/usr/bin/env python3
"""Build a Linux x86_64 bundle using Python 3.11+ and the standard library."""

import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import urllib.error
import urllib.request


REPOSITORY = Path(__file__).resolve().parent.parent


def verify_sha256(path, expected):
    with path.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").hexdigest()
    if actual != expected:
        raise ValueError(f"SHA-256 mismatch for {path}: expected {expected}, got {actual}")
    return actual


def validate_elf(data, label):
    """Check ELF64 little-endian executable/shared-object headers for Linux x86_64."""
    valid = (len(data) >= 64 and data[:7] == b"\x7fELF\x02\x01\x01"
             and data[7] in (0, 3))  # System V or Linux ABI, as used by GNU Linux.
    if valid:
        kind, machine, version = struct.unpack_from("<HHI", data, 16)
        valid = (kind in (2, 3) and machine == 62 and version == 1
                 and struct.unpack_from("<H", data, 52)[0] == 64)
    if not valid:
        raise ValueError(f"{label} must be a Linux x86_64 ELF executable")


def read_carboxyl(archive_path, metadata):
    """Verify the pinned archive and read its one expected file without extracting paths."""
    verify_sha256(archive_path, metadata["archive_sha256"])
    with tarfile.open(archive_path, "r:gz") as archive:
        members = archive.getmembers()
        if (len(members) != 1 or members[0].name != metadata["archive_member"]
                or not members[0].isfile()):
            raise ValueError("Carboxyl archive must contain only its expected regular-file member")
        member = members[0]
        if not member.mode & 0o111:
            raise ValueError("Carboxyl archive member must be executable")
        with archive.extractfile(member) as source:
            data = source.read()
    validate_elf(data, "Carboxyl")
    return data


def cached_carboxyl(cache_dir, metadata):
    """Download to a temporary file, verifying the digest before publishing the cache entry."""
    cache_dir.mkdir(parents=True, exist_ok=True)
    destination = cache_dir / f"carboxyl-v{metadata['version']}-x86_64-linux.tar.gz"
    if destination.exists():
        verify_sha256(destination, metadata["archive_sha256"])
        return destination
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(dir=cache_dir, prefix=destination.name + ".",
                                         suffix=".part", delete=False) as output:
            temporary_path = Path(output.name)
            request = urllib.request.Request(metadata["archive_url"], headers={"User-Agent": "desktop-tui-packager"})
            with urllib.request.urlopen(request, timeout=60) as source:
                shutil.copyfileobj(source, output)
        verify_sha256(temporary_path, metadata["archive_sha256"])
        os.replace(temporary_path, destination)
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)
    return destination


def write_atomic(path, data):
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix=path.name + ".",
                                         suffix=".part", delete=False) as output:
            temporary_path = Path(output.name)
            output.write(data)
        os.replace(temporary_path, path)
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)


def build_package(repository, desktop_binary, browser_archive, output_dir, metadata):
    """Write a normalized tar+gzip bundle and its external SHA256SUMS manifest."""
    if not desktop_binary.is_file() or not desktop_binary.stat().st_mode & 0o111:
        raise ValueError(f"Desktop-TUI must be a regular executable file: {desktop_binary}")
    desktop = desktop_binary.read_bytes()
    validate_elf(desktop, "Desktop-TUI")
    browser = read_carboxyl(browser_archive, metadata)
    with (repository / "Cargo.toml").open("rb") as source:
        version = tomllib.load(source)["package"]["version"]
    if not isinstance(version, str) or not re.fullmatch(r"[0-9A-Za-z][0-9A-Za-z.+-]*", version):
        raise ValueError("Cargo package version cannot be used as a bundle filename")
    prefix = f"desktop-tui-{version}-linux-x86_64"
    entries = {
        f"{prefix}/desktop-tui": (desktop, 0o755),
        f"{prefix}/libexec/carboxyl": (browser, 0o755),
        f"{prefix}/licenses/Desktop-TUI-LICENSE": ((repository / "LICENSE").read_bytes(), 0o644),
        f"{prefix}/licenses/Carboxyl-LICENSE": ((repository / "packaging/carboxyl/LICENSE").read_bytes(), 0o644),
        f"{prefix}/licenses/WezTerm-LICENSE": ((repository / "vendor/wezterm-term/LICENSE.md").read_bytes(), 0o644),
        f"{prefix}/licenses/Local-dependencies.md": ((repository / "vendor/README.md").read_bytes(), 0o644),
        f"{prefix}/licenses/AppCUI-Cargo.toml": ((repository / "vendor/appcui/Cargo.toml").read_bytes(), 0o644),
        f"{prefix}/licenses/Carboxyl-PROVENANCE.json": (
            (repository / "packaging/carboxyl/PROVENANCE.json").read_bytes(), 0o644),
        f"{prefix}/README.txt": ((repository / "packaging/linux/README.txt").read_bytes(), 0o644),
    }
    for shortcut in sorted((repository / "shortcuts").rglob("*.toml")):
        name = shortcut.relative_to(repository).as_posix()
        entries[f"{prefix}/{name}"] = (shortcut.read_bytes(), 0o644)
    # Directory entries make permissions independent of the unpacker's umask.
    for name in list(entries):
        for parent in Path(name).parents:
            if parent != Path("."):
                entries.setdefault(parent.as_posix() + "/", (None, 0o755))
    entries.setdefault(f"{prefix}/shortcuts/", (None, 0o755))
    output_dir.mkdir(parents=True, exist_ok=True)
    destination = output_dir / f"{prefix}.tar.gz"
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(dir=output_dir, prefix=destination.name + ".",
                                         suffix=".part", delete=False) as raw:
            temporary_path = Path(raw.name)
            # Omit the temporary filename and timestamp from the gzip header.
            with gzip.GzipFile(filename="", mode="wb", compresslevel=6, fileobj=raw, mtime=0) as compressed:
                with tarfile.open(fileobj=compressed, mode="w|", format=tarfile.PAX_FORMAT) as archive:
                    for name, (data, mode) in sorted(entries.items()):
                        member = tarfile.TarInfo(name)
                        member.mode = mode
                        member.type = tarfile.DIRTYPE if data is None else tarfile.REGTYPE
                        member.size = 0 if data is None else len(data)
                        # TarInfo defaults uid/gid/mtime to zero and uname/gname to empty.
                        archive.addfile(member, None if data is None else io.BytesIO(data))
        with temporary_path.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        os.replace(temporary_path, destination)
        write_atomic(output_dir / "SHA256SUMS", f"{digest}  {destination.name}\n".encode("ascii"))
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--desktop-binary", type=Path,
                        help="use an existing Linux x86_64 executable instead of building Desktop-TUI")
    parser.add_argument("--carboxyl-archive", type=Path,
                        help="use the pinned Carboxyl release archive locally (SHA-256 is still verified)")
    parser.add_argument("--output-dir", type=Path, default=REPOSITORY / "target/dist",
                        help="bundle and SHA256SUMS directory (default: target/dist)")
    args = parser.parse_args()
    try:
        metadata = json.loads((REPOSITORY / "packaging/carboxyl/PROVENANCE.json").read_text())
        desktop = args.desktop_binary
        if desktop is None:
            subprocess.run(["cargo", "build", "--locked", "--release"], cwd=REPOSITORY, check=True)
            target = Path(os.environ.get("CARGO_TARGET_DIR", "target"))
            if not target.is_absolute():
                target = REPOSITORY / target
            desktop = target / "release/desktop-tui"
        browser = args.carboxyl_archive
        if browser is None:
            browser = cached_carboxyl(REPOSITORY / "target/package-cache", metadata)
        output = build_package(REPOSITORY, desktop, browser, args.output_dir, metadata)
    except (OSError, ValueError, KeyError, tarfile.TarError, urllib.error.URLError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Packaging failed: {error}\n")
    print(output)
    print(output.parent / "SHA256SUMS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
