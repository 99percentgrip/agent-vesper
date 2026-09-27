#!/usr/bin/env python3
"""Real TUI process proof for Settings → Providers → Manage authentication.

Isolated HOME plus explicit signed-out vaults. Does not read or write the
developer keyring, and does not save a new credential.
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from settings_pty import Host


def signed_out_vault(path: Path, provider: str, account: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(
            {
                "credentials": {
                    provider: {account: json.dumps({"mode": "signed-out"})}
                }
            }
        )
    )
    path.chmod(0o600)


def focused(host) -> str:
    for row in host.screen:
        line = "".join(row)
        if "›" in line and "│" in line:
            return line.strip()
    raise AssertionError(f"no focused row\n{host.text()}")


def arrow_navigation(binary: str) -> None:
    """Arrow-only proof of the recorded miss. Does not press M."""
    import tempfile

    with tempfile.TemporaryDirectory(prefix="vesper-settings-auth-arrows-") as temporary:
        root = Path(temporary)
        signed_out_vault(root / "config" / "xai-credentials.json", "xai", "native-auth")
        signed_out_vault(root / "config" / "openai-credentials.json", "openai", "native-auth")
        host = Host(
            binary,
            root,
            {
                "AGENT_VESPER_HOME": str(root / "state"),
                "AGENT_VESPER_PROVIDER": "zai",
                "AGENT_VESPER_XAI_CREDENTIALS_PATH": str(root / "config" / "xai-credentials.json"),
                "AGENT_VESPER_OPENAI_CREDENTIALS_PATH": str(
                    root / "config" / "openai-credentials.json"
                ),
                "AGENT_VESPER_LMSTUDIO_CREDENTIALS_PATH": str(
                    root / "config" / "lmstudio-credentials.json"
                ),
                "AGENT_VESPER_VRO_ENABLED": "0",
                "AGENT_VESPER_SANDBOX": "off",
            },
        )
        try:
            host.wait("Start coding")
            host.key("s")
            host.click("Providers")
            host.wait("Manage authentication · zai")
            line = focused(host)
            assert "zai" in line and "Manage authentication" not in line, line
            host.key("\x1b[A")
            host.key("\x1b[A")
            line = focused(host)
            assert "xai" in line and "Manage authentication" not in line, line
            host.key("\x1b[B")
            line = focused(host)
            assert "Manage authentication" in line and "xai" in line, host.text()
            assert "zai" not in line, host.text()
            host.key("\r")
            host.wait("xAI / Grok")
            assert "Z.ai GLM" not in host.text()
            host.key("\x1b")
            host.wait("Manage authentication · xai")
            assert "xai" in focused(host)
            host.key("\r")
            host.wait("xAI / Grok")
            host.key("\x1b")
            host.wait("Providers")
            host.key("\x1b[B")
            assert "zai" in focused(host) and "Manage authentication" not in focused(host)
            host.key("\x1b[B")
            assert "Manage authentication" in focused(host) and "zai" in focused(host)
            host.key("\r")
            host.wait("Z.ai GLM")
            host.key("\x1b")
            for _ in range(12):
                if "lmstudio" in focused(host) and "Manage authentication" not in focused(host):
                    break
                host.key("\x1b[A")
            else:
                raise AssertionError(host.text())
            host.key("\x1b[B")
            assert "Manage authentication" in focused(host) and "lmstudio" in focused(host)
            host.key("\r")
            host.wait("LM Studio")
            host.key("\x1b")
            host.key("\x1b[B")
            assert "openai" in focused(host) and "Manage authentication" not in focused(host)
            host.key("\x1b[B")
            assert "Manage authentication" in focused(host) and "openai" in focused(host)
            host.key("\r")
            host.wait("OpenAI")
            host.key("\x1b")
            host.wait("zai (active)")
            assert not (root / "state" / "provider").exists()
            host.key("\x1b[B")
            assert "xai" in focused(host) and "Manage authentication" not in focused(host)
            host.key("m")
            host.wait("xAI / Grok")
            host.key("\x1b")
            assert not (root / "state" / "provider").exists()
        finally:
            host.close()
    print(
        "PASS: arrow Down from xAI opened xAI authentication; first, middle, and last providers stayed attached to their own actions; M still opened xAI; Back did not activate another provider."
    )


def main(binary: str) -> None:
    import tempfile

    with tempfile.TemporaryDirectory(prefix="vesper-settings-auth-pty-") as temporary:
        root = Path(temporary)
        signed_out_vault(root / "config" / "xai-credentials.json", "xai", "native-auth")
        signed_out_vault(root / "config" / "openai-credentials.json", "openai", "native-auth")
        host = Host(
            binary,
            root,
            {
                "AGENT_VESPER_HOME": str(root / "state"),
                "AGENT_VESPER_PROVIDER": "zai",
                "AGENT_VESPER_XAI_CREDENTIALS_PATH": str(root / "config" / "xai-credentials.json"),
                "AGENT_VESPER_OPENAI_CREDENTIALS_PATH": str(
                    root / "config" / "openai-credentials.json"
                ),
                "AGENT_VESPER_LMSTUDIO_CREDENTIALS_PATH": str(
                    root / "config" / "lmstudio-credentials.json"
                ),
                "AGENT_VESPER_VRO_ENABLED": "0",
                "AGENT_VESPER_SANDBOX": "off",
            },
        )
        try:
            host.wait("Start coding")
            host.key("s")
            host.click("Providers")
            host.wait("Manage authentication")
            host.click("xai")
            host.click("Manage authentication · xai")
            host.wait("Authentication")
            screen = host.text()
            assert "Grok account" in screen, screen
            assert "API key" in screen, screen
            assert "Browser sign-in" in screen, screen
            assert "Device-code" in screen, screen
            host.click("xAI API key")
            host.click("Enter or replace API key")
            host.wait("masked")
            host.key("secret-canary")
            assert "secret-canary" not in host.text()
            host.key("\x1b")
            host.wait("Authentication")
            host.key("\x1b")
            host.wait("Manage authentication")
            host.click("openai")
            host.click("Manage authentication · openai")
            host.wait("Authentication")
            screen = host.text()
            assert "ChatGPT subscription" in screen, screen
            assert "API key" in screen, screen
            host.click("ChatGPT subscription")
            host.wait("Device-code")
            host.key("\x1b")
            host.wait("Providers")
            host.click("Cancel")
            host.wait("Settings")
            host.key("\x1b")
            host.wait("Start coding")
            assert not (root / "state" / "provider").exists()
            assert "secret-canary" not in host.raw
        finally:
            host.close()
    print(
        "PASS: TUI process Settings → Providers → Manage authentication showed xAI and OpenAI methods, masked input, and Cancel back to chat without saving."
    )


if __name__ == "__main__":
    arrow_navigation(sys.argv[1])
    main(sys.argv[1])
