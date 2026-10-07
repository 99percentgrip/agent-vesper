import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("prepare-linux-apt.py")
spec = importlib.util.spec_from_file_location("prepare_linux_apt", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class AptPreparationTests(unittest.TestCase):
    def test_both_failed_setup_paths_use_shared_preparation_and_bounds(self):
        root = SCRIPT.parent
        sandbox = (root / 'workflows/platform-foundation.yml').read_text()
        browser = (root / 'workflows/web-driver.yml').read_text()
        for workflow in (sandbox, browser):
            self.assertIn('sudo --preserve-env=GITHUB_ACTIONS,RUNNER_OS python3 .github/prepare-linux-apt.py', workflow)
        self.assertIn('timeout 120 sudo apt-get install -y bubblewrap', sandbox)
        self.assertIn('timeout 240', browser)
        self.assertNotIn('echo "http://archive.ubuntu.com/ubuntu"', sandbox)
        self.assertIn('python3 .github/test_prepare_linux_apt.py', (root / 'workflows/ci.yml').read_text())

    def test_https_preserves_mirror_paths_sources_and_idempotence(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "apt.conf.d").mkdir()
            (root / "sources.list.d").mkdir()
            (root / "apt-mirrors.txt").write_text("http://azure.archive.ubuntu.com/ubuntu\n")
            (root / "apt-mirrors-security.txt").write_text("http://security.ubuntu.com/ubuntu\n")
            source = root / "sources.list.d" / "ubuntu.sources"
            source.write_text("URIs: mirror+file:/etc/apt/apt-mirrors.txt http://azure.archive.ubuntu.com/ubuntu\nSuites: noble noble-updates\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n")
            third_party = root / "sources.list.d" / "vendor.list"
            third_party.write_text("deb https://packages.microsoft.com/ubuntu/24.04/prod noble main\n")
            module.prepare(root)
            self.assertEqual((root / "apt-mirrors.txt").read_text(), "https://archive.ubuntu.com/ubuntu\n")
            self.assertEqual((root / "apt-mirrors-security.txt").read_text(), "https://security.ubuntu.com/ubuntu\n")
            self.assertIn("mirror+file:/etc/apt/apt-mirrors.txt", source.read_text())
            self.assertIn("https://archive.ubuntu.com/ubuntu", source.read_text())
            self.assertIn("Signed-By:", source.read_text())
            self.assertEqual(third_party.read_text(), "deb https://packages.microsoft.com/ubuntu/24.04/prod noble main\n")
            before = {p: p.read_bytes() for p in root.rglob("*") if p.is_file()}
            module.prepare(root)
            self.assertEqual(before, {p: p.read_bytes() for p in root.rglob("*") if p.is_file()})
            config = (root / "apt.conf.d" / "99-vesper-ci-acquire").read_text()
            for key in ("ForceIPv4", "Retries", "http::Timeout", "https::Timeout"):
                self.assertIn(key, config)

    def test_refuses_outside_hosted_linux(self):
        for action, platform in (("", "Linux"), ("true", "Windows")):
            env = dict(os.environ, GITHUB_ACTIONS=action, RUNNER_OS=platform)
            result = subprocess.run([sys.executable, str(SCRIPT)], env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("requires a GitHub-hosted Linux runner", result.stderr)

    def test_arm64_retains_ports_archive_and_unknown_architecture_refuses(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'apt.conf.d').mkdir()
            (root / 'apt-mirrors.txt').write_text('http://ports.ubuntu.com/ubuntu-ports\n')
            before = (root / 'apt-mirrors.txt').read_bytes()
            with self.assertRaisesRegex(ValueError, 'unsupported'):
                module.prepare(root, 'unknown')
            self.assertEqual((root / 'apt-mirrors.txt').read_bytes(), before)
            module.prepare(root, 'aarch64')
            self.assertEqual((root / 'apt-mirrors.txt').read_text(), 'https://ports.ubuntu.com/ubuntu-ports\n')
            self.assertFalse((root / 'apt-mirrors-security.txt').exists())


if __name__ == "__main__":
    unittest.main()
