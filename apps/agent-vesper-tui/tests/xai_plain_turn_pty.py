#!/usr/bin/env python3
"""Real TUI process smoke for the native xAI Grok-session composition."""
import http.server
import json
from pathlib import Path
import socketserver
import sys
import tempfile
import threading

sys.path.insert(0, str(Path(__file__).parent))
from settings_pty import Host


class Handler(http.server.BaseHTTPRequestHandler):
    requests = []

    def log_message(self, *_args):
        pass

    def do_GET(self):
        assert self.path == "/language-models"
        assert self.headers["X-XAI-Token-Auth"] == "xai-grok-cli"
        body = b'{"models":[{"id":"grok-4.7"}]}'
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        length = int(self.headers["content-length"])
        request = json.loads(self.rfile.read(length))
        Handler.requests.append(request)
        names = {tool.get("name") for tool in request["tools"]}
        assert {"read_file", "run_command", "update_plan"} <= names
        assert request["reasoning"]["effort"] == "high"
        body = (
            'data: {"type":"response.output_text.delta","output_index":0,'
            '"delta":"hello from xAI TUI process"}\n\n'
            'data: {"type":"response.completed","response":{}}\n\n'
        ).encode()
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main(binary):
    server = socketserver.TCPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    with tempfile.TemporaryDirectory(prefix="vesper-xai-tui-") as temporary:
        root = Path(temporary)
        host = Host(binary, root, {
            "AGENT_VESPER_HOME": str(root / "state"),
            "AGENT_VESPER_PROVIDER": "xai",
            "AGENT_VESPER_XAI_TEST_URL": f"http://127.0.0.1:{server.server_address[1]}/responses",
            "AGENT_VESPER_XAI_TEST_STALE_HOSTED": "1",
            "AGENT_VESPER_VRO_ENABLED": "0",
            "AGENT_VESPER_SANDBOX": "off",
            "NO_PROXY": "127.0.0.1",
            "no_proxy": "127.0.0.1",
        })
        try:
            host.wait("Start coding")
            host.key("\r")
            host.wait("grok-4.7")
            host.key("hello")
            host.key("\r")
            host.wait("hello from xAI TUI process")
            assert len(Handler.requests) == 1
        finally:
            host.close()
            server.shutdown()
            server.server_close()
    print("PASS: TUI process plain hello reached xAI Grok-session transport once")


if __name__ == "__main__":
    main(sys.argv[1])
