#!/usr/bin/env python3
"""Credential-free native landing and all-provider Settings regression.

No provider requests, real credentials, or invoking-user state are permitted.
"""
from pathlib import Path
import sys
import tempfile

from settings_pty import Host
from settings_auth_pty import signed_out_vault


def run(binary):
    for selected in ("zai", "openai", "xai", "lmstudio"):
        with tempfile.TemporaryDirectory(prefix="vesper-first-launch-") as temporary:
            root = Path(temporary)
            config = root / "config"
            config.mkdir()
            # A deliberately unreadable vault blocks native-keyring fallback.
            # Z.ai has no signed-out tombstone; never consult the caller's store.
            glm = config / "zai.json"
            glm.write_text('{"credentials":null}')
            glm.chmod(0o600)
            for provider in ("openai", "xai", "lmstudio"):
                signed_out_vault(config / f"{provider}.json", provider, "native-auth")
            host = Host(binary, root, {
                "ZAI_API_KEY": "", "Z_AI_API_KEY": "",
                "AGENT_VESPER_HOME": str(root / "state"),
                "AGENT_VESPER_PROVIDER": selected,
                "AGENT_VESPER_CREDENTIALS_PATH": str(glm),
                "AGENT_VESPER_OPENAI_CREDENTIALS_PATH": str(config / "openai.json"),
                "AGENT_VESPER_XAI_CREDENTIALS_PATH": str(config / "xai.json"),
                "AGENT_VESPER_LMSTUDIO_CREDENTIALS_PATH": str(config / "lmstudio.json"),
                "AGENT_VESPER_VRO_ENABLED": "0",
            })
            try:
                host.wait("Start coding", timeout=10)
                assert "Authentication" not in host.text(), host.text()
                host.key("s")
                host.choose("Providers")
                for provider, title in (("zai", "Z.ai GLM"), ("openai", "OpenAI"),
                                        ("xai", "xAI / Grok"), ("lmstudio", "LM Studio")):
                    host.choose(provider)
                    host.choose(f"Manage authentication · {provider}")
                    host.wait(title)
                    host.wait("Authentication")
                    host.key("\x1b")
                    host.wait("Manage authentication")
                    assert host.child.poll() is None, "authentication cancellation exited TUI"
                host.key("\x1b")
                host.key("\x1b")
                host.wait("Start coding")
                assert not (root / "state" / "provider").exists()
                host.key("q")
                host.child.wait(timeout=10)
                assert host.child.returncode == 0, host.raw[-2000:]
            finally:
                host.close()
            print(f"PASS: signed-out {selected}: landing, four providers, auth Back, clean Quit")


if __name__ == "__main__":
    run(str(Path(sys.argv[1]).resolve()))
