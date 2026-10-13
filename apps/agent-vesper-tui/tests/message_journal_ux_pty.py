#!/usr/bin/env python3
"""Show more, restore, copy, and FIFO against the canonical MessageJournal."""
import http.server
import json
from pathlib import Path
import socketserver
import sys
import tempfile
import threading

sys.path.insert(0, str(Path(__file__).parent))
from settings_pty import Host

BODY = "\n".join([
    "# MARKER-TITLE",
    "",
    "```rust",
    "fn kept() {}",
    "```",
    "",
    "    indented-keep 💡",
    "middle-line",
    "MARKER-TAIL-UNIQUE",
])


class Handler(http.server.BaseHTTPRequestHandler):
    requests = []
    first_seen = threading.Event()
    release = threading.Event()
    hold_next = 1

    def log_message(self, *_args):
        pass

    def do_GET(self):
        body = b'{"models":[{"id":"grok-4.7"}]}'
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["content-length"])))
        self.requests.append(request)
        number = len(self.requests)
        if number == Handler.hold_next:
            self.first_seen.set()
            assert self.release.wait(45), "held request was not released"
        reply = f"reply-{number}"
        payload = (
            'data: {"type":"response.output_text.delta","output_index":0,'
            f'"delta":{json.dumps(reply)}}}\n\n'
            'data: {"type":"response.completed","response":{}}\n\n'
        ).encode()
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("content-length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


class Server(socketserver.ThreadingMixIn, socketserver.TCPServer):
    allow_reuse_address = True
    daemon_threads = True


def click_text(host, label):
    host.wait(label)
    for y, row in enumerate(host.screen):
        line = "".join(row)
        if label in line and "focused" not in line:
            x = line.index(label)
            host.key(f"\x1b[<0;{x + 2};{y + 1}M")
            return
    raise AssertionError(f"no clickable {label!r}\n{host.text()}")


def paste(host, text):
    host.write(b"\x1b[200~" + text.encode() + b"\x1b[201~")
    host.pump(0.4)


def user_rows(screen):
    return ["".join(row).strip() for row in screen if "".join(row).strip().startswith("›")]


def run(binary):
    Handler.requests = []
    Handler.first_seen.clear()
    Handler.release.clear()
    Handler.hold_next = 1
    server = Server(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    port = server.server_address[1]
    host = None
    try:
        with tempfile.TemporaryDirectory(prefix="vesper-journal-ux-") as temporary:
            root = Path(temporary)
            env = {
                "AGENT_VESPER_HOME": str(root / "state"),
                "AGENT_VESPER_SESSION_ROOT": str(root / "sessions"),
                "AGENT_VESPER_CHECKPOINT_ROOT": str(root / "checkpoints"),
                "AGENT_VESPER_PROVIDER": "xai",
                "AGENT_VESPER_XAI_TEST_URL": f"http://127.0.0.1:{port}/responses",
                "AGENT_VESPER_VRO_ENABLED": "0",
                "NO_PROXY": "127.0.0.1",
                "no_proxy": "127.0.0.1",
            }
            host = Host(binary, root, env)
            host.wait("Start coding")
            host.key("\r")
            host.wait("grok-4.7")
            paste(host, BODY)
            host.key("\r")
            assert Handler.first_seen.wait(20), "provider did not see the long prompt"
            host.pump(0.6)
            assert "Show more" in host.text(), host.text()
            assert "MARKER-TAIL-UNIQUE" not in host.text(), host.text()
            assert "MARKER-TITLE" in host.text(), host.text()
            assert "/paste" not in host.text(), host.text()
            host.key("\t")
            host.wait("Show more is focused")
            host.key("\r")
            host.wait("Show less")
            host.pump(0.3)
            plain = host.text()
            for marker in ("MARKER-TAIL-UNIQUE", "fn kept", "indented-keep", "💡", "middle-line", "Show less"):
                assert marker in plain, marker + "\n" + plain
            assert "    indented-keep 💡" in host.raw
            host.key("\r")
            host.wait("Prompt collapsed")
            host.pump(0.2)
            assert "MARKER-TAIL-UNIQUE" not in host.text(), host.text()
            assert "Show less" not in host.text()
            host.key("\t")
            host.wait("Show more is focused")
            host.key("z")
            host.pump(0.2)
            assert "Show less" not in host.text(), host.text()
            assert "Prompt expanded" not in host.text(), host.text()
            host.key("\x7f")
            click_text(host, "Copy full prompt")
            host.wait("Copied")
            assert (root / "checkpoints" / "clipboard.log").read_text() == BODY + "\n"
            Handler.release.set()
            host.wait("reply-1", 30)
            host.key("/quit\r")
            assert host.wait_exit(10) == 0
            host.close()
            host = None
            records = list((root / "sessions").glob("*.json"))
            assert len(records) == 1, records
            record = json.loads(records[0].read_text())
            assert record["message_journal"]["messages"][0]["text"] == BODY
            assert all("MARKER-TAIL-UNIQUE" not in line for line in record["display_transcript"])
            Handler.first_seen.clear()
            Handler.release.clear()
            Handler.hold_next = 2
            host = Host(binary, root, env)
            host.wait("Start coding")
            host.key("\r")
            host.wait("grok-4.7")
            host.key(f"/history {record['session_id']}\r")
            host.wait("Resumed")
            host.pump(0.4)
            assert "MARKER-TAIL-UNIQUE" not in host.text(), host.text()
            assert "Show more" in host.text(), host.text()
            host.key("\t")
            host.wait("Show more is focused")
            host.key("\r")
            host.wait("MARKER-TAIL-UNIQUE")
            assert "    indented-keep 💡" in host.raw
            host.key("\r")
            host.wait("Prompt collapsed")
            click_text(host, "Copy full prompt")
            host.wait("Copied")
            raw_log = (root / "checkpoints" / "clipboard.log").read_bytes()
            assert raw_log.endswith((BODY + "\n").encode()), raw_log
            host.key("Hold this turn.\r")
            assert Handler.first_seen.wait(20)
            host.pump(0.3)
            host.key("QUEUE-ONE-EXACT\r")
            host.wait("QUEUE-ONE-EXACT")
            host.key("QUEUE-TWO-EXACT")
            host.key("\t")
            host.wait("QUEUE-TWO-EXACT")
            host.pump(0.4)
            lines = ["".join(row) for row in host.screen]
            def rows(label):
                return [line for line in lines if label in line and "QUEUED" not in line and "/queue" not in line]
            assert len(rows("QUEUE-ONE-EXACT")) == 1, host.text()
            assert len(rows("QUEUE-TWO-EXACT")) == 1, host.text()
            assert "Show less" not in host.text(), host.text()
            assert "Prompt expanded" not in host.text(), host.text()
            live = json.loads(records[0].read_text())
            assert [m["text"] for m in live["message_journal"]["messages"][-2:]] == [
                "QUEUE-ONE-EXACT",
                "QUEUE-TWO-EXACT",
            ]
            assert [m["state"] for m in live["message_journal"]["messages"][-2:]] == ["Pending", "Pending"]
            displayed = [line for line in live["display_transcript"] if line.startswith("user:")]
            assert displayed.count("user: QUEUE-ONE-EXACT") == 1
            assert displayed.count("user: QUEUE-TWO-EXACT") == 1
            Handler.release.set()
            host.wait("reply-4", 30)
            host.pump(0.6)
            final = json.loads(records[0].read_text())
            assert [m["text"] for m in final["message_journal"]["messages"]] == [
                BODY,
                "Hold this turn.",
                "QUEUE-ONE-EXACT",
                "QUEUE-TWO-EXACT",
            ]
            assert [m["state"] for m in final["message_journal"]["messages"]] == ["Settled"] * 4
            wire = []
            for request in Handler.requests:
                pieces = [
                    piece.get("text")
                    for item in request["input"]
                    if item.get("role") == "user"
                    for piece in item.get("content", [])
                ]
                wire.append(pieces[-1] if pieces else None)
            assert wire == [BODY, "Hold this turn.", "QUEUE-ONE-EXACT", "QUEUE-TWO-EXACT"], wire
            assert len([line for line in final["display_transcript"] if line.startswith("user:")]) == 4
    finally:
        if host is not None and host.child.poll() is None:
            host.close()
        server.shutdown()


if __name__ == "__main__":
    run(sys.argv[1])
