"""Offline guards only; no package, device, provider or user-state access."""
import os
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import verify_native_voice


class NativeVoiceGuards(unittest.TestCase):
    def test_refuses_local_execution_before_any_installation(self):
        with patch.dict(os.environ, {}, clear=True), patch.object(verify_native_voice, "run") as command:
            with self.assertRaisesRegex(RuntimeError, "requires a disposable GitHub runner"):
                verify_native_voice.main()
            command.assert_not_called()

    def test_failed_command_does_not_count_as_a_voice_receipt(self):
        with patch.object(verify_native_voice.subprocess, "run") as command, patch("builtins.print"):
            command.return_value.stdout = "NATIVE VOICE DEPENDENCIES VERIFIED"
            command.return_value.returncode = 1
            with self.assertRaisesRegex(RuntimeError, "failed"):
                verify_native_voice.run(["controlled-fixture"])

    def test_prerequisite_integrity_and_bounds_refuse_before_execution(self):
        for payload, size, digest in [(b"oversized", 1, "0" * 64), (b"bad", 3, "0" * 64)]:
            with tempfile.TemporaryDirectory() as root:
                stream = io.BytesIO(payload)
                stream.geturl = lambda: "https://fixture.invalid/package"
                with patch.object(verify_native_voice.urllib.request, "urlopen", return_value=stream):
                    with self.assertRaisesRegex(RuntimeError, "bounds|integrity"):
                        verify_native_voice.download_verified("https://fixture.invalid/package", Path(root) / "package", size, digest)


if __name__ == "__main__":
    unittest.main()
