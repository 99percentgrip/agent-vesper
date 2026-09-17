#!/usr/bin/env python3
"""Vesper Bridge host-side driver for the Resolve free-edition worker.

Sends typed commands via cmd.json, reads results via result.json, monitors
liveness via beat. This is the host half of the paste-once-per-launch route.
"""
import json
import os
import sys
import time
from pathlib import Path

IPC = Path("/home/Alex/Videos/vb-fixture/ipc")
CMD = IPC / "cmd.json"
RES = IPC / "result.json"
BEAT = IPC / "beat"

ALLOWED_OPS = {
    "status", "fixture_import", "timeline_from_fixture",
    "render_draft", "render_start", "render_status",
    "set_marker", "get_state",
}


def worker_alive(max_age_s: float = 5.0) -> bool:
    try:
        age = time.time() - BEAT.stat().st_mtime
        return age < max_age_s
    except OSError:
        return False


def send(op: str, args: dict | None = None, timeout_s: float = 30.0) -> dict:
    if op not in ALLOWED_OPS:
        return {"ok": False, "data": f"host refused op {op!r}"}
    if not worker_alive():
        return {"ok": False, "data": "worker not alive (no recent beat)"}
    seq = int(time.time() * 1000) % 10_000_000
    RES.unlink(missing_ok=True)
    CMD.write_text(json.dumps({"id": seq, "op": op, "args": args or {}}))
    t0 = time.time()
    while time.time() - t0 < timeout_s:
        if RES.exists():
            try:
                out = json.loads(RES.read_text())
                if out.get("id") == seq:
                    RES.unlink(missing_ok=True)
                    return out
            except (json.JSONDecodeError, OSError):
                pass
        time.sleep(0.1)
    return {"ok": False, "data": f"timeout after {timeout_s}s waiting for {op}"}


def main() -> int:
    if len(sys.argv) < 2:
        print(f"usage: {sys.argv[0]} <op> [json-args]", file=sys.stderr)
        print(f"  ops: {sorted(ALLOWED_OPS)}", file=sys.stderr)
        return 2
    op = sys.argv[1]
    args = json.loads(sys.argv[2]) if len(sys.argv) > 2 else {}
    print(json.dumps(send(op, args), indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
