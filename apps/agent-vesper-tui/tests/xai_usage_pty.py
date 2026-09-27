#!/usr/bin/env python3
"""Real TUI process proof that /usage reads Grok billing and does not infer."""
import json
from http.server import BaseHTTPRequestHandler
from pathlib import Path
import socketserver
import sys
import tempfile
import threading

sys.path.insert(0, str(Path(__file__).parent))
from settings_pty import Host


class Handler(BaseHTTPRequestHandler):
    paths = []

    def log_message(self, *_args):
        pass

    def do_POST(self):
        Handler.paths.append(f"POST {self.path}")
        self.send_error(500)

    def do_GET(self):
        Handler.paths.append(self.path)
        if self.path == "/language-models":
            body = b'{"models":[{"id":"grok-4.7"}]}'
        elif self.path == "/user":
            assert self.headers["X-XAI-Token-Auth"] == "xai-grok-cli"
            assert self.headers["x-grok-client-identifier"] == "agent-vesper"
            body = b'{"userId":"account-a"}'
        elif self.path == "/billing?format=credits":
            assert self.headers["x-userid"] == "account-a"
            body = json.dumps(
                {
                    "subscriptionTier": "SuperGrok",
                    "config": {
                        "creditUsagePercent": 10,
                        "currentPeriod": {
                            "type": "USAGE_PERIOD_TYPE_WEEKLY",
                            "end": "2026-06-08T00:00:00Z",
                        },
                        "prepaidBalance": {"val": 1250},
                        "productUsage": [
                            {"product": "PRODUCT_GROK_BUILD", "usagePercent": 4}
                        ],
                    },
                }
            ).encode()
        else:
            raise AssertionError(self.path)
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main(binary):
    server = socketserver.TCPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    with tempfile.TemporaryDirectory(prefix="vesper-xai-usage-") as temporary:
        root = Path(temporary)
        host = Host(
            binary,
            root,
            {
                "AGENT_VESPER_HOME": str(root / "state"),
                "AGENT_VESPER_PROVIDER": "xai",
                "AGENT_VESPER_XAI_TEST_URL": f"http://127.0.0.1:{server.server_address[1]}/responses",
                "AGENT_VESPER_XAI_TEST_STALE_HOSTED": "1",
                "AGENT_VESPER_VRO_ENABLED": "0",
                "AGENT_VESPER_SANDBOX": "off",
                "NO_PROXY": "127.0.0.1",
                "no_proxy": "127.0.0.1",
            },
        )
        try:
            host.wait("Start coding")
            host.key("\r")
            host.wait("grok-4.7")
            host.key("/usage\r")
            host.wait("90% left", timeout=20)
            text = host.text()
            for expected in (
                "Weekly allowance",
                "Extra usage credits",
                "$12.50",
                "Grok Build",
                "SuperGrok",
                "Context window:",
            ):
                assert expected in text, text
            assert "account-a" not in text
            assert not any(path.startswith("POST ") for path in Handler.paths)
            assert "/user" in Handler.paths
            assert "/billing?format=credits" in Handler.paths
        finally:
            host.close()
            server.shutdown()
            server.server_close()
    print("PASS: TUI /usage queried Grok billing without inference")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: xai_usage_pty.py <agent-vesper-tui>")
    main(sys.argv[1])
