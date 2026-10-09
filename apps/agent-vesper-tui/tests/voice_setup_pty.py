"""Native voice setup consent/decline and draft preservation; never confirms install."""
from pathlib import Path
import sys
import tempfile
from settings_pty import Host


def run(binary):
    with tempfile.TemporaryDirectory(prefix="vesper-voice-consent-") as temporary:
        root = Path(temporary)
        host = Host(binary, root)
        try:
            host.wait("Start coding")
            host.key("s")
            host.choose("Voice")
            host.choose("Voice conversation")
            host.wait("Voice conversation · current ON")
            host.choose("Set up local voice dependencies")
            host.wait("Your OS may request administrator approval")
            # Structurally avoids the confirmed setup branch on every platform.
            host.choose("Cancel")
            host.wait("Voice conversation · current ON")
            assert not (root / "data/agent-vesper/voice-tools").exists()
            assert not (root / "data/agent-vesper/voice-venv").exists()
            host.key("\x1b")
            host.key("\x1b")
            host.choose("Discard changes")
            host.wait("Start coding")
            host.key("s")
            host.choose("Voice")
            host.wait("Voice conversation · current OFF")
            host.key("\x1b")
            host.key("\x1b")
            host.wait("Start coding")
            host.key("q")
            host.child.wait(timeout=10)
            assert host.child.returncode == 0
            print("PASS: native voice setup consent, decline, draft preserved and discard restored OFF")
        finally:
            host.close()


if __name__ == "__main__":
    run(str(Path(sys.argv[1]).resolve()))
