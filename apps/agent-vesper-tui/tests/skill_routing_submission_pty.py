#!/usr/bin/env python3
"""Paste ordinary prose through the real TUI and prove two provider submissions."""
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
        reply = f"ordinary routing reply {len(Handler.requests)}"
        body = (
            'data: {"type":"response.output_text.delta","output_index":0,'
            f'"delta":{json.dumps(reply)}}}\n\n'
            'data: {"type":"response.completed","response":{}}\n\n'
        ).encode()
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def request_contains(request, needle):
    if isinstance(request, str):
        return needle in request
    if isinstance(request, list):
        return any(request_contains(item, needle) for item in request)
    if isinstance(request, dict):
        return any(request_contains(value, needle) for value in request.values())
    return False


def main(binary):
    binary = str(Path(binary).resolve())
    Handler.requests = []
    server = socketserver.TCPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    with tempfile.TemporaryDirectory(prefix="vesper-skill-routing-tui-") as temporary:
        root = Path(temporary)
        host = Host(binary, root, {
            "AGENT_VESPER_HOME": str(root / "state"),
            "AGENT_VESPER_PROVIDER": "xai",
            "AGENT_VESPER_XAI_TEST_URL": f"http://127.0.0.1:{server.server_address[1]}/responses",
            "AGENT_VESPER_VRO_ENABLED": "0",
            "AGENT_VESPER_SANDBOX": "off",
            "NO_PROXY": "127.0.0.1",
            "no_proxy": "127.0.0.1",
        })
        ordinary = (
            "Use the actual repository review links.\n"
            "Explain tools, permissions, skills and memory."
        )
        try:
            host.wait("Start coding")
            host.key("\r")
            host.wait("grok-4.7")
            # Bracketed paste is the actual Event::Paste path; embedded LF must
            # remain one submission rather than triggering an early Enter.
            host.key("\x1b[200~" + ordinary + "\x1b[201~")
            host.wait("Pasted Content")
            assert not Handler.requests
            host.key("\r")
            host.wait("ordinary routing reply 1")
            assert len(Handler.requests) == 1, Handler.requests
            assert request_contains(Handler.requests[0], ordinary), Handler.requests[0]
            assert "skill routing failed" not in host.text()

            host.key("Confirm the session accepted a later turn.")
            host.key("\r")
            host.wait("ordinary routing reply 2")
            assert len(Handler.requests) == 2, Handler.requests
            assert request_contains(
                Handler.requests[1], "Confirm the session accepted a later turn."
            )
        finally:
            host.close()
            server.shutdown()
            server.server_close()
    print("PASS: bracketed paste reached the TUI provider once, preserved prompt bytes, and accepted a later turn")


if __name__ == "__main__":
    main(sys.argv[1])
