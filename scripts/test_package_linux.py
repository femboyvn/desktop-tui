#!/usr/bin/env python3
"""Hardware-free package checks: python3 -B -m unittest discover -s scripts -v."""

import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("package-linux.py")


def elf(machine=62, osabi=0):
    identification = b"\x7fELF\x02\x01\x01" + bytes([osabi]) + bytes(8)
    return identification + struct.pack("<HHIQQQIHHHHHH", 3, machine, 1, 0, 0, 0, 0, 64, 0, 0, 0, 0, 0)


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue(SCRIPT.is_file(), "The Linux package builder is not implemented")
        spec = importlib.util.spec_from_file_location("package_linux", SCRIPT)
        self.package = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.package)
        self.directory = tempfile.TemporaryDirectory(prefix="desktop package test ")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.payload = elf() + b"browser payload"
        self.archive = self.make_archive("browser.tar.gz")
        self.metadata = {
            "version": "0.2.0",
            "archive_member": "carboxyl-x86_64-unknown-linux-gnu",
            "archive_sha256": hashlib.sha256(self.archive.read_bytes()).hexdigest(),
            "archive_url": self.archive.as_uri(),
        }

    def make_archive(self, filename, members=None):
        path = self.root / filename
        if members is None:
            members = [("carboxyl-x86_64-unknown-linux-gnu", tarfile.REGTYPE, 0o755, self.payload)]
        with tarfile.open(path, "w:gz") as archive:
            for name, kind, mode, data in members:
                member = tarfile.TarInfo(name)
                member.type = kind
                member.mode = mode
                member.size = len(data) if kind == tarfile.REGTYPE else 0
                if kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
                    member.linkname = "../outside"
                archive.addfile(member, io.BytesIO(data) if member.isfile() else None)
        return path

    def make_repository(self):
        repo = self.root / "repository with spaces"
        (repo / "shortcuts").mkdir(parents=True)
        (repo / "packaging/carboxyl").mkdir(parents=True)
        (repo / "packaging/linux").mkdir(parents=True)
        (repo / "vendor/wezterm-term").mkdir(parents=True)
        (repo / "vendor/appcui").mkdir(parents=True)
        (repo / "Cargo.toml").write_text('[package]\nname = "desktop-tui"\nversion = "1.2.3"\n')
        (repo / "LICENSE").write_text("desktop license\n")
        (repo / "shortcuts/carboxyl.toml").write_text('command = "carboxyl"\n')
        (repo / "packaging/carboxyl/LICENSE").write_text("browser license\n")
        (repo / "packaging/carboxyl/PROVENANCE.json").write_text(json.dumps(self.metadata))
        (repo / "packaging/linux/README.txt").write_text("bundle instructions\n")
        (repo / "vendor/wezterm-term/LICENSE.md").write_text("terminal core license\n")
        (repo / "vendor/README.md").write_text("local dependency provenance\n")
        (repo / "vendor/appcui/Cargo.toml").write_text('authors = ["AppCUI author"]\nlicense = "MIT"\n')
        desktop = repo / "desktop input"
        desktop.write_bytes(elf() + b"desktop payload")
        desktop.chmod(0o755)
        return repo, desktop

    def test_cli_help_does_not_build_or_download(self):
        result = subprocess.run([sys.executable, str(SCRIPT), "--help"], cwd=self.root,
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--desktop-binary", result.stdout)
        self.assertIn("--carboxyl-archive", result.stdout)

    def test_tampered_archive_is_rejected_before_opening(self):
        self.archive.write_bytes(b"tampered archive")
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            self.package.read_carboxyl(self.archive, self.metadata)

    def test_exact_regular_executable_member_is_read(self):
        self.assertEqual(self.package.read_carboxyl(self.archive, self.metadata), self.payload)

    def test_unsafe_members_are_rejected_without_writing_files(self):
        member = "carboxyl-x86_64-unknown-linux-gnu"
        sandbox = self.root / "extraction sandbox"
        sandbox.mkdir()
        # A future unsafe extraction regression must not write into the checkout.
        runner = """
import importlib.util, json, sys
spec = importlib.util.spec_from_file_location('package_linux', sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
from pathlib import Path
try:
    module.read_carboxyl(Path(sys.argv[2]), json.loads(sys.argv[3]))
except ValueError as error:
    print(error)
    sys.exit(3)
"""
        cases = [
            [("../outside", tarfile.REGTYPE, 0o755, self.payload)],
            [(str(self.root / "outside"), tarfile.REGTYPE, 0o755, self.payload)],
            [(member, tarfile.SYMTYPE, 0o755, b"")],
            [(member, tarfile.LNKTYPE, 0o755, b"")],
            [(member, tarfile.DIRTYPE, 0o755, b"")],
            [(member, tarfile.REGTYPE, 0o755, self.payload)] * 2,
            [(member, tarfile.REGTYPE, 0o755, self.payload),
             ("../outside", tarfile.REGTYPE, 0o755, b"unsafe")],
        ]
        for index, members in enumerate(cases):
            with self.subTest(index=index):
                archive = self.make_archive(f"unsafe-{index}.tar.gz", members)
                metadata = dict(self.metadata, archive_sha256=hashlib.sha256(archive.read_bytes()).hexdigest())
                before = set(self.root.iterdir())
                result = subprocess.run([sys.executable, "-B", "-c", runner, str(SCRIPT),
                                         str(archive), json.dumps(metadata)], cwd=sandbox,
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 3, result.stderr)
                self.assertIn("member", result.stdout)
                self.assertEqual(set(self.root.iterdir()), before)
                self.assertEqual(list(sandbox.iterdir()), [])
                self.assertFalse((self.root / "outside").exists())

    def test_nonexecutable_browser_is_rejected(self):
        archive = self.make_archive("not-executable.tar.gz", [
            ("carboxyl-x86_64-unknown-linux-gnu", tarfile.REGTYPE, 0o644, self.payload)])
        metadata = dict(self.metadata, archive_sha256=hashlib.sha256(archive.read_bytes()).hexdigest())
        with self.assertRaisesRegex(ValueError, "executable"):
            self.package.read_carboxyl(archive, metadata)

    def test_wrong_architecture_browser_is_rejected_after_checksum_verification(self):
        archive = self.make_archive("wrong-browser.tar.gz", [
            ("carboxyl-x86_64-unknown-linux-gnu", tarfile.REGTYPE, 0o755, elf(machine=183))])
        metadata = dict(self.metadata, archive_sha256=hashlib.sha256(archive.read_bytes()).hexdigest())
        with self.assertRaisesRegex(ValueError, "Linux x86_64 ELF"):
            self.package.read_carboxyl(archive, metadata)

    def test_wrong_architecture_or_nonlinux_elf_is_rejected(self):
        for payload in (b"#!/bin/sh\nexit 0\n", elf(machine=183), elf(osabi=9), elf()[:20]):
            with self.subTest(payload=payload):
                with self.assertRaisesRegex(ValueError, "Linux x86_64 ELF"):
                    self.package.validate_elf(payload, "fixture")

    def test_nonexecutable_desktop_is_rejected(self):
        repo, desktop = self.make_repository()
        desktop.chmod(0o644)
        with self.assertRaisesRegex(ValueError, "executable"):
            self.package.build_package(repo, desktop, self.archive, self.root / "dist", self.metadata)
        self.assertFalse((self.root / "dist/SHA256SUMS").exists())

    def test_wrong_architecture_desktop_is_rejected(self):
        repo, desktop = self.make_repository()
        desktop.write_bytes(elf(machine=183))
        with self.assertRaisesRegex(ValueError, "Linux x86_64 ELF"):
            self.package.build_package(repo, desktop, self.archive, self.root / "dist", self.metadata)

    def test_bundle_layout_modes_contents_and_checksum(self):
        repo, desktop = self.make_repository()
        output = self.root / "output with spaces"
        bundle = self.package.build_package(repo, desktop, self.archive, output, self.metadata)
        self.assertEqual(bundle, output / "desktop-tui-1.2.3-linux-x86_64.tar.gz")
        prefix = "desktop-tui-1.2.3-linux-x86_64/"
        expected = {
            prefix: None,
            prefix + "desktop-tui": desktop.read_bytes(),
            prefix + "libexec/": None,
            prefix + "libexec/carboxyl": self.payload,
            prefix + "shortcuts/": None,
            prefix + "shortcuts/carboxyl.toml": b'command = "carboxyl"\n',
            prefix + "licenses/": None,
            prefix + "licenses/Desktop-TUI-LICENSE": b"desktop license\n",
            prefix + "licenses/Carboxyl-LICENSE": b"browser license\n",
            prefix + "licenses/WezTerm-LICENSE": b"terminal core license\n",
            prefix + "licenses/Local-dependencies.md": b"local dependency provenance\n",
            prefix + "licenses/AppCUI-Cargo.toml": b'authors = ["AppCUI author"]\nlicense = "MIT"\n',
            prefix + "licenses/Carboxyl-PROVENANCE.json": (repo / "packaging/carboxyl/PROVENANCE.json").read_bytes(),
            prefix + "README.txt": b"bundle instructions\n",
        }
        with tarfile.open(bundle) as archive:
            members = archive.getmembers()
            names = [item.name + ("/" if item.isdir() else "") for item in members]
            self.assertEqual(names, sorted(expected))
            for name, item in zip(names, members):
                self.assertEqual((item.uid, item.gid, item.uname, item.gname, item.mtime), (0, 0, "", "", 0))
                want = expected[name]
                if want is None:
                    self.assertTrue(item.isdir())
                    self.assertEqual(item.mode, 0o755)
                else:
                    self.assertTrue(item.isfile())
                    self.assertEqual(archive.extractfile(item).read(), want)
                    self.assertEqual(item.mode, 0o755 if name.endswith(("/desktop-tui", "/libexec/carboxyl")) else 0o644)
        digest = hashlib.sha256(bundle.read_bytes()).hexdigest()
        self.assertEqual((output / "SHA256SUMS").read_text(), f"{digest}  {bundle.name}\n")
        self.assertFalse(list(output.glob("*.part")))

    def test_archive_is_reproducible_despite_input_mtimes_or_output_directory(self):
        repo, desktop = self.make_repository()
        first = self.package.build_package(repo, desktop, self.archive, self.root / "first", self.metadata)
        os.utime(desktop, (1234567890, 1234567890))
        os.utime(repo / "shortcuts/carboxyl.toml", (234567890, 234567890))
        second = self.package.build_package(repo, desktop, self.archive, self.root / "second", self.metadata)
        self.assertEqual(first.read_bytes(), second.read_bytes())

    def test_download_is_verified_cached_and_reusable_without_source(self):
        cached = self.package.cached_carboxyl(self.root / "cache", self.metadata)
        self.assertEqual(cached.read_bytes(), self.archive.read_bytes())
        self.archive.unlink()
        self.assertEqual(self.package.cached_carboxyl(self.root / "cache", self.metadata), cached)
        self.assertFalse(list((self.root / "cache").glob("*.part")))

    def test_tampered_download_is_not_promoted_to_cache(self):
        self.archive.write_bytes(b"bad download")
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            self.package.cached_carboxyl(self.root / "cache", self.metadata)
        self.assertEqual(list((self.root / "cache").iterdir()), [])

    def test_tampered_cache_is_rejected(self):
        cached = self.package.cached_carboxyl(self.root / "cache", self.metadata)
        cached.write_bytes(b"tampered cache")
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            self.package.cached_carboxyl(self.root / "cache", self.metadata)

    def test_cli_explicit_binary_skips_build_and_rejects_unpinned_browser(self):
        _, desktop = self.make_repository()
        environment = dict(os.environ, PATH="")
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--desktop-binary", str(desktop),
             "--carboxyl-archive", str(self.archive), "--output-dir", str(self.root / "output")],
            cwd=self.root, env=environment, capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn("SHA-256 mismatch", result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        self.assertFalse((self.root / "output").exists())

    def test_cli_default_build_uses_locked_release_at_repository_root(self):
        cargo = self.root / "cargo"
        record = self.root / "cargo-record.json"
        cargo.write_text(f"#!{sys.executable}\nimport json, os, sys\n"
                         f"with open({str(record)!r}, 'w') as output:\n"
                         "    json.dump({'args': sys.argv[1:], 'cwd': os.getcwd()}, output)\n"
                         "sys.exit(9)\n")
        cargo.chmod(0o755)
        result = subprocess.run([sys.executable, str(SCRIPT)], cwd=self.root,
                                env=dict(os.environ, PATH=str(self.root)), capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(json.loads(record.read_text()), {
            "args": ["build", "--locked", "--release"], "cwd": str(SCRIPT.parent.parent),
        })
        self.assertIn("Packaging failed", result.stderr)
        self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
