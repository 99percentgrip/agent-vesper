"""Offline PE import parser and release portability gate regressions."""

from pathlib import Path
import struct
import tempfile
import unittest

from windows_package_audit import EXECUTABLES, audit, pe_imports


def fixture(dll="kernel32.dll", delay=False):
    data = bytearray(2048)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<HH", data, 0x84, 0x8664, 1)
    struct.pack_into("<H", data, 0x94, 240)
    optional = 0x98
    struct.pack_into("<H", data, optional, 0x20B)
    struct.pack_into("<Q", data, optional + 24, 0x140000000)
    struct.pack_into("<I", data, optional + 60, 0x200)
    struct.pack_into("<I", data, optional + 108, 16)
    struct.pack_into("<IIII", data, optional + 240 + 8, 0x600, 0x1000, 0x600, 0x200)
    directory, width = (13, 32) if delay else (1, 20)
    struct.pack_into("<II", data, optional + 112 + directory * 8, 0x1000, width * 2)
    if delay:
        struct.pack_into("<II", data, 0x200, 1, 0x1100)
    else:
        struct.pack_into("<I", data, 0x20C, 0x1100)
    encoded = dll.encode("ascii") + b"\0"
    data[0x300:0x300 + len(encoded)] = encoded
    return data


class PackageAuditTests(unittest.TestCase):
    def test_release_and_prerequisite_keep_real_package_guards(self):
        root = Path(__file__).resolve().parents[1]
        config = (root / ".cargo/config.toml").read_text()
        target = config.split("[target.x86_64-pc-windows-msvc]", 1)[1].split("\n[", 1)[0]
        self.assertIn('rustflags = ["-C", "target-feature=+crt-static"]', target)
        ci = (root / ".github/workflows/ci.yml").read_text()
        self.assertIn("scripts/windows_package_audit.py target/x86_64-pc-windows-msvc/release", ci)
        self.assertEqual(ci.count("scripts\\test_install_windows_release.ps1"), 2)
        release = (root / ".github/workflows/release.yml").read_text()
        self.assertLess(release.index("python scripts/windows_package_audit.py $staging"),
                        release.index("Compress-Archive -Path $staging"))
        installer = (root / "scripts/install.ps1").read_text()
        self.assertLess(installer.index("0xC0000135: required DLL missing"),
                        installer.index("New-Item -ItemType Directory -Path $InstallDir"))

    def test_reads_normal_and_delay_imports(self):
        for delay in (False, True):
            self.assertEqual(pe_imports(fixture(delay=delay)), ["kernel32.dll"])

    def test_runtime_dependency_refuses_for_every_shipped_executable(self):
        for executable in EXECUTABLES:
            for dll in ("VCRUNTIME140.dll", "MSVCP140.dll", "ucrtbase.dll",
                        "api-ms-win-crt-runtime-l1-1-0.dll"):
                with tempfile.TemporaryDirectory() as root:
                    bundle = Path(root)
                    for name in EXECUTABLES:
                        (bundle / name).write_bytes(fixture(dll if name == executable else "kernel32.dll", delay=True))
                    with self.assertRaisesRegex(ValueError, "separately installed runtime"):
                        audit(bundle)

    def test_all_four_system_only_executables_pass(self):
        with tempfile.TemporaryDirectory() as root:
            for name in EXECUTABLES:
                (Path(root) / name).write_bytes(fixture())
            self.assertEqual(set(audit(Path(root))), set(EXECUTABLES))

    def test_missing_executable_cannot_pass(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaises(FileNotFoundError):
                audit(Path(root))

    def test_malformed_or_virtual_imports_fail_closed(self):
        bad = fixture()
        struct.pack_into("<I", bad, 0x20C, 0x9000)
        for data in (b"", b"MZ", fixture()[:300], bad):
            with self.assertRaises(ValueError):
                pe_imports(data)

    def test_unterminated_directory_is_rejected(self):
        data = fixture()
        struct.pack_into("<I", data, 0x98 + 112 + 8 + 4, 20)
        with self.assertRaisesRegex(ValueError, "unterminated import directory"):
            pe_imports(data)


if __name__ == "__main__":
    unittest.main()
