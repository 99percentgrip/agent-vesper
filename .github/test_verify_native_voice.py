"""Offline guards only; no package, device, provider or user-state access."""
import os
import io
import json
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


class RecognitionReceipts(unittest.TestCase):
    def test_requires_actual_fixed_phrase_twice_from_persistent_sidecar(self):
        values = [{"ready": True}, {"index": 0, "text": "This is a speech recognition test. Hello world.", "seconds": 4},
                  {"done": True, "chunks": 1}]
        receipt = "\n".join(json.dumps(v) for v in values + values[1:])
        verify_native_voice.verify_recognition_receipt(receipt)
        invalid = [values[:1], values, values + [{"error": "transcription failed"}],
                   values + values[1:][::-1],
                   [values[0], {"index": 0, "text": "", "seconds": 4}, values[2]] + values[1:],
                   [values[0], {"index": 0, "text": "unrelated phrase", "seconds": 4}, values[2]] + values[1:]]
        for case in invalid:
            with self.subTest(case=case), self.assertRaisesRegex(RuntimeError, "recognition"):
                verify_native_voice.verify_recognition_receipt("\n".join(json.dumps(v) for v in case))
        for misleading in ["NATIVE VOICE DEPENDENCIES VERIFIED", "voice=am_michael samples=53200", "not JSON"]:
            with self.assertRaisesRegex(RuntimeError, "recognition"):
                verify_native_voice.verify_recognition_receipt(misleading)


if __name__ == "__main__":
    unittest.main()
