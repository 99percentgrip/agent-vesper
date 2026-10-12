#!/usr/bin/env python3
"""Real terminal queue/paste lifecycle against a bounded loopback provider."""
import http.server
import json
import os
import signal
from pathlib import Path
import socketserver
import sys
import tempfile
import threading
import time

sys.path.insert(0, str(Path(__file__).parent))
from settings_pty import Host
from skill_routing_submission_pty import request_contains


class Handler(http.server.BaseHTTPRequestHandler):
    requests = []
    first_seen = threading.Event()
    release = threading.Event()
    hold_next = 1
    fail_next = 0
    tool_next = 0

    def log_message(self, *_args):
        pass

    def do_GET(self):
        assert self.path == "/language-models", self.path
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
            assert self.release.wait(45), "terminal did not release held provider request"
        if number == Handler.fail_next:
            body = b'{"error":{"message":"fixture request rejected"}}'
            self.send_response(400)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        reply = f"queue fixture reply {number}"
        body = (
            'data: {"type":"response.output_text.delta","output_index":0,'
            f'"delta":{json.dumps(reply)}}}\n\n'
            'data: {"type":"response.completed","response":{}}\n\n'
        ).encode()
        if number == Handler.tool_next:
            arguments = json.dumps({"command": "echo queue-approval-proof > approval-proof.txt"})
            events = [
                {"type": "response.output_item.added", "output_index": 0, "item": {"type": "function_call", "call_id": "call_approval", "name": "run_command", "arguments": ""}},
                {"type": "response.function_call_arguments.delta", "output_index": 0, "delta": arguments},
                {"type": "response.output_item.done", "output_index": 0, "item": {"type": "function_call", "call_id": "call_approval", "name": "run_command", "arguments": arguments}},
                {"type": "response.completed", "response": {}},
            ]
            body = "".join("data: " + json.dumps(event) + "\n\n" for event in events).encode()
        try:
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            # This fixture deliberately terminates/cancels a held client.
            pass



class Server(socketserver.ThreadingMixIn, socketserver.TCPServer):
    daemon_threads = True


def command(host, text):
    host.key(text)
    host.key("\r")


def run(binary):
    Handler.requests = []
    Handler.first_seen.clear()
    Handler.release.clear()
    Handler.hold_next = 1
    Handler.fail_next = 0
    server = Server(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    with tempfile.TemporaryDirectory(prefix="vesper-queue-pty-") as temporary:
        root = Path(temporary)
        environment = {
            "AGENT_VESPER_HOME": str(root / "state"),
            "AGENT_VESPER_SESSION_ROOT": str(root / "sessions"),
            "AGENT_VESPER_PROVIDER": "xai",
            "AGENT_VESPER_XAI_TEST_URL": f"http://127.0.0.1:{server.server_address[1]}/responses",
            "AGENT_VESPER_VRO_ENABLED": "0",
            "NO_PROXY": "127.0.0.1", "no_proxy": "127.0.0.1",
        }
        host = Host(binary, root, environment)
        paste = "SPEC_START\n" + "Windows Unicode specification 💡\n" * 160
        second_paste = "EVIDENCE_START\ncomplete second block"
        literal_path = root / "literal.png"
        literal_path.write_bytes(b"terminal text reference, not image data")
        expected = "Before " + paste + " between " + str(literal_path) + second_paste + " after"
        try:
            host.wait("Start coding")
            host.key("\r")
            host.wait("grok-4.7")
            command(host, "Hold the first turn until fixture releases it.")
            assert Handler.first_seen.wait(20)
            host.key("Before ")
            host.key("\x1b[200~" + paste + "\x1b[201~")
            host.wait("Pasted Content")
            assert "SPEC_START" not in host.text(), "payload flooded composer"
            host.key(" between ")
            host.key("\x1b[200~" + str(literal_path) + "\x1b[201~")
            host.key("\x1b[200~" + second_paste + "\x1b[201~")
            host.key(" after")
            host.key("\r")
            host.wait("QUEUED 1")
            command(host, "The second future turn.")
            host.wait("QUEUED 2")
            command(host, "/queue")
            host.wait("2 pending")
            assert len(Handler.requests) == 1, "queued text steered or ran concurrently"
            for text in ["/unregistered-command", "release patch"]:
                host.key(text)
                host.key("\t")
                host.wait("Native commands cannot be queued")
                assert text in host.text(), "queue shortcut lost rejected command draft"
                assert len(Handler.requests) == 1, "command shortcut reached the provider"
                if text == "release patch":
                    # The private workspace has no Git repository. Native RRC
                    # admission must reject it without touching invoking state.
                    host.key("\r")
                    host.wait("release admission failed")
                    assert len(Handler.requests) == 1, "active native release fell through to provider work"
                else:
                    host.key("\r")
                    host.wait("Unknown command")
                    assert len(Handler.requests) == 1, "unknown slash command reached the provider"
            command(host, "/queue")
            host.wait("2 pending")
            command(host, "/paste p1")
            host.wait("SPEC_START")
            host.key("\x1b")
            host.wait("2 pending")
            command(host, "Discard this pending turn.")
            host.wait("QUEUED 3")
            command(host, "/queue drop m4")
            host.wait("Queue updated")
            assert len(Handler.requests) == 1, "queue command sent provider input"
            Handler.release.set()
            host.wait("queue fixture reply 3", 30)
            assert len(Handler.requests) == 3, Handler.requests
            assert request_contains(Handler.requests[1], expected), Handler.requests[1]
            texts = [piece.get("text") for item in Handler.requests[1]["input"]
                     if item.get("role") == "user" for piece in item.get("content", [])]
            assert texts.count(expected) == 1, "paste body was altered or duplicated on the wire"
            assert request_contains(Handler.requests[2], "The second future turn."), Handler.requests[2]
            assert not request_contains(Handler.requests[2], "Discard this pending turn.")
            host.pump(.6)
            assert len(Handler.requests) == 3, "redraw dispatched a duplicate"
            command(host, "/queue")
            host.wait("0 pending")
            records = list((root / "sessions").glob("*.json"))
            assert len(records) == 1, records
            record = json.loads(records[0].read_text())
            journal = record["message_journal"]
            assert journal["pending"] == [] and journal["active"] is None, journal
            message = next(m for m in journal["messages"] if m["id"] == "m2")
            assert message["text"] == expected and len(message["pastes"]) == 2, message
            assert message["state"] == "Settled", message
            assert all("SPEC_START" not in line for line in record["display_transcript"]), "normal transcript expanded payload"
            command(host, "/paste m2")
            host.wait("SPEC_START")
            # Explicit normal exit saves the inspection identity; restart must
            # restore it without replaying settled provider work.
            host.key("\x1b")
            command(host, "/quit")
            assert host.wait_exit(10) == 0, "normal terminal exit failed"
            host.close()
            host = Host(binary, root, environment)
            host.wait("Start coding")
            host.key("\r")
            host.wait("grok-4.7")
            command(host, "/history " + record["session_id"])
            host.wait("Resumed")
            command(host, "/paste p2")
            host.wait("EVIDENCE_START")
            assert len(Handler.requests) == 3, "session resume replayed settled work"
            host.key("\x1b")
            command(host, "/quit")
            assert host.wait_exit(10) == 0, "normal terminal exit failed"
            host.close()
            host = None
            owned = root / "approval"
            owned.mkdir()
            first = len(Handler.requests) + 1
            Handler.hold_next = first
            Handler.tool_next = first
            Handler.first_seen.clear()
            Handler.release.clear()
            scoped_env = dict(environment, AGENT_VESPER_HOME=str(owned / "state"),
                              AGENT_VESPER_SESSION_ROOT=str(owned / "sessions"))
            host = Host(binary, owned, scoped_env)
            host.wait("Start coding")
            host.key("\r")
            host.wait("grok-4.7")
            command(host, "Ask approval for the private fixture command.")
            assert Handler.first_seen.wait(20)
            command(host, "Future after tool approval.")
            host.wait("QUEUED 1")
            command(host, "Drop this during approval.")
            host.wait("QUEUED 2")
            host.key("Draft kept after commands.")
            Handler.release.set()
            host.wait("Allow once", 25)
            host.key("\x10")
            host.wait("commands only")
            host.key("queue")
            host.key("\r")
            host.wait("2 pending")
            command(host, "/queue drop m3")
            host.wait("Queue updated")
            assert len(Handler.requests) == first, "approval commands dispatched a turn"
            host.key("\x1b")
            host.wait("Allow once")
            host.key("\r")
            host.wait(f"queue fixture reply {first + 2}", 30)
            assert len(Handler.requests) == first + 2, Handler.requests
            assert (owned / "approval-proof.txt").read_text().strip() == "queue-approval-proof", "approved tool did not complete"
            assert request_contains(Handler.requests[-1], "Future after tool approval.")
            assert not request_contains(Handler.requests[-1], "Drop this during approval.")
            assert "Draft kept after commands." in host.text(), "approval command view lost draft"
            # Original draft remains unsubmitted; clear it explicitly before quit.
            host.key("\x7f" * len("Draft kept after commands."))
            command(host, "/quit")
            assert host.wait_exit(10) == 0, "normal terminal exit failed"
            host.close()
            host = None
            Handler.tool_next = 0
            for scenario in ["crash", "cancel", "error"]:
                owned = root / scenario
                owned.mkdir()
                next_request = len(Handler.requests) + 1
                Handler.hold_next = next_request
                Handler.fail_next = next_request if scenario == "error" else 0
                Handler.first_seen.clear()
                Handler.release.clear()
                scoped_env = dict(environment, AGENT_VESPER_HOME=str(owned / "state"),
                                  AGENT_VESPER_SESSION_ROOT=str(owned / "sessions"))
                host = Host(binary, owned, scoped_env)
                host.wait("Start coding")
                host.key("\r")
                host.wait("grok-4.7")
                command(host, f"Uncertain {scenario} active turn.")
                assert Handler.first_seen.wait(20)
                command(host, f"Future after {scenario}.")
                host.wait("QUEUED 1")
                records = list((owned / "sessions").glob("*.json"))
                assert len(records) == 1
                before = json.loads(records[0].read_text())
                assert before["message_journal"]["active"] == "m1"
                assert before["message_journal"]["pending"] == ["m2"]
                if scenario == "crash":
                    host.close()
                    host = Host(binary, owned, scoped_env)
                    host.wait("Start coding")
                    host.key("\r")
                    host.wait("grok-4.7")
                    command(host, "/history " + before["session_id"])
                    host.wait("paused")
                elif scenario == "cancel":
                    host.key("\x03")
                    host.wait("paused")
                    host.wait("READY")
                else:
                    Handler.release.set()
                    host.wait("paused")
                    host.wait("READY")
                host.pump(.5)
                assert len(Handler.requests) == next_request, f"{scenario} blindly advanced/replayed"
                command(host, "/queue")
                host.wait("1 pending")
                command(host, "/queue resume")
                host.wait(f"queue fixture reply {next_request + 1}", 25)
                assert len(Handler.requests) == next_request + 1
                assert request_contains(Handler.requests[-1], f"Future after {scenario}.")
                if scenario == "crash":
                    # Historical context is retained; only the newest user turn
                    # is executed. The POST count above excludes a replay.
                    users = [item for item in Handler.requests[-1]["input"] if item.get("role") == "user"]
                    assert request_contains(users[-1], "Future after crash."), users
                    assert not request_contains(users[-1], "Uncertain crash active turn."), users
                Handler.release.set()
                settled = json.loads(records[0].read_text())["message_journal"]
                assert settled["messages"][0]["state"] == "Interrupted", settled
                assert settled["messages"][1]["state"] == "Settled", settled
                command(host, "/quit")
                assert host.wait_exit(10) == 0, "normal terminal exit failed"
                host.close()
                host = None
        finally:
            Handler.release.set()
            if host is not None:
                host.close()
            server.shutdown()
            server.server_close()
            thread.join(5)
    print("PASS: real PTY mixed paste ordering, Enter queue, FIFO, native release and unknown slash command bypass, Tab command draft retention, drop, full inspection, compact transcript, stable persistence, real tool approval command access and draft restoration, tool-heavy settlement, cancellation/error pauses, crash recovery and explicit safe resume")


if __name__ == "__main__":
    if os.name != "nt":
        def timeout(_signal, _frame):
            raise TimeoutError("queue PTY fixture exceeded its bounded lifetime")
        signal.signal(signal.SIGALRM, timeout)
        signal.signal(signal.SIGTERM, timeout)
        signal.alarm(240)
    run(str(Path(sys.argv[1]).resolve()))
