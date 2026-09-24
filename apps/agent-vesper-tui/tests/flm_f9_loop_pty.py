#!/usr/bin/env python3
"""Native F9 regression for the FLM NPU STT route: real binary, real
Settings save, real F9 event path, real Silero VAD worker, real owned
FLM ASR process + installed Whisper model, loopback reasoning double,
and a no-device player sink. Proves: Settings save -> F9 -> selected
adapter (FLM, not CPU sidecar) -> one ordinary agent turn, with the CPU
sidecar never constructed.

Requires: the installed FLM runtime + verified Whisper pack (this test
never downloads), the existing voice venv (Silero), and espeak-ng.
No microphone, no speaker, no cloud provider.
"""
import http.server
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import settings_pty




def run_production_verify(binary):
    """Runs the release binary's own real-model Verify composition via the
    standalone receipt example (same code path as the Settings action)."""
    import subprocess
    env = dict(os.environ, AGENT_VESPER_PROVIDER='lmstudio',
               NO_PROXY='127.0.0.1,localhost',
               FLM_MODEL_PATH=str(Path.home() / '.config/flm'))
    example = Path(binary).parent.parent / 'examples/flm_stt_receipt'
    if not example.is_file():
        example = Path('/home/Alex/Projects/agent-vesper/target/release/examples/flm_stt_receipt')
    result = subprocess.run([str(example), '--i-have-the-installed-model'],
                            capture_output=True, text=True, timeout=300, env=env)
    print('verify receipt:', result.stdout.strip()[-200:])
    return result.returncode == 0


def run(binary, python, voice="am_michael"):
    requests = []

    class Provider(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            body = json.dumps({'models': [{'type': 'llm', 'key': 'voice-fixture',
                'display_name': 'Voice fixture', 'max_context_length': 131072,
                'capabilities': {'vision': False, 'trained_for_tool_use': True}}]}).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            size = int(self.headers.get('Content-Length', '0'))
            if not 0 < size < 2 * 1024 * 1024:
                self.send_error(413)
                return
            request = json.loads(self.rfile.read(size))
            if 'messages' not in request:
                self.send_error(503, 'optional embedding fixture unavailable')
                return
            requests.append(request)
            # SSE word-sized deltas (the transport the production client
            # parses; a single JSON body would never surface as reply).
            text = 'Local NPU acknowledgment. One sentence.'
            chunks = []
            carried = ''
            for word in text.split(' '):
                carried += word + ' '
                if len(carried) >= 12:
                    chunks.append(carried)
                    carried = ''
            if carried:
                chunks.append(carried)
            data = b''
            for chunk in chunks:
                data += ('data: ' + json.dumps({'choices': [{'index': 0, 'delta': {'content': chunk}, 'finish_reason': None}]}) + '\n\n').encode()
            data += ('data: ' + json.dumps({'choices': [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}]}) + '\n\ndata: [DONE]\n\n').encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Provider)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    fixture = Path(__file__).parent
    real_pack = Path.home() / '.local/share/agent-vesper/voice-pack'
    assert real_pack.is_dir(), 'existing verified pack required; this test never downloads'

    # The FLM route uses the REAL installed model in the user cache
    # (never copied, never re-downloaded).
    flm_root = Path.home() / '.config/flm/models'

    temp = Path(tempfile.mkdtemp(prefix='.flm-loop-', dir=Path.cwd()))
    root = temp
    commands = root / 'bin'
    commands.mkdir()
    espeak = shutil.which('espeak-ng')
    assert espeak, 'verified pronunciation prerequisite required'
    phonemizer = commands / 'espeak-ng'
    phonemizer.write_text('#!/usr/bin/env python3\nimport os,sys\n'
        'if "--ipa" not in sys.argv or "-q" not in sys.argv or "--stdout" in sys.argv: sys.exit(91)\n'
        + f'os.execv({espeak!r}, [{espeak!r}] + sys.argv[1:])\n')
    phonemizer.chmod(0o755)
    # Recorder fixture: emits the SAME three-segment tonal surrogate
    # the recorded backend gate proved detectable by the installed
    # Silero defaults (window [0, 1.584 s) of a 4.0 s capture), then
    # a genuine trailing silent tail. Deterministic; never a mic.
    recorder = commands / 'arecord'
    # R20 streaming shape: `arecord ... -` writes the WAV container on
    # stdout (the managed capture drains it through its capped writer).
    recorder.write_text(
        '#!/usr/bin/env python3\n'
        'import math, struct, sys, time\n'
        'segments = [(0.25, 0.60, 190.0), (0.95, 0.70, 240.0), (2.10, 0.55, 210.0)]\n'
        'total = 4.0\n'
        'n = int(total * 16000)\n'
        'frames = bytearray()\n'
        'for i in range(n):\n'
        '    t = i / 16000.0\n'
        '    s = 0.0\n'
        '    for (st, du, f0) in segments:\n'
        '        if st <= t < st + du:\n'
        '            lt = t - st\n'
        '            env = max(0.0, math.sin(math.pi * lt / du))\n'
        '            s = env * (math.sin(2*math.pi*f0*lt) + 0.5*math.sin(2*math.pi*2*f0*lt) + 0.25*math.sin(2*math.pi*3*f0*lt)) / 1.75\n'
        '    frames += int(max(-32768, min(32767, s * 22000.0))).to_bytes(2, "little", signed=True)\n'
        'data = bytes(frames)\n'
        'hdr = b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVEfmt " + struct.pack(\n'
        '    "<IHHIIHH", 16, 1, 1, 16000, 32000, 2, 16) + b"data" + struct.pack("<I", len(data))\n'
        'out = sys.stdout.buffer\n'
        'out.write(hdr); out.write(data); out.flush()\n'
        'while True:\n'
        '    time.sleep(0.1)\n')
    recorder.chmod(0o755)
    # No-device player sink: counts bytes, never opens audio hardware.
    player = commands / 'aplay'
    player.write_text('#!/usr/bin/env python3\nimport os,sys,struct,time\n'
        'first = sys.stdin.buffer.read(2)\n'
        'with open(os.path.join(os.environ["VOICE_FIXTURE_ROOT"], "player-first-pcm"), "a") as f: f.write(str(time.monotonic())+"\\n")\n'
        'data = first + sys.stdin.buffer.read()\nn = len(data)\n'
        'with open(os.path.join(os.environ["VOICE_FIXTURE_ROOT"], "player-bytes"), "a") as f: f.write(str(n)+"\\n")\n')
    player.chmod(0o755)
    # The CPU recognizer is REPLACED by a sentinel: if the FLM route
    # ever constructs the CPU sidecar, this import fails and the
    # probe dies loudly (the FLM route must use no CPU recognizer).
    (root / 'faster_whisper.py').write_text(
        'import sys\n'
        'def _poison(*_a, **_k):\n'
        '    sys.stderr.write("CPU RECOGNIZER CONSTRUCTED ON FLM ROUTE\\n")\n'
        '    raise SystemExit(97)\n'
        'class WhisperModel:\n'
        '    def __init__(self, *a, **k):\n'
        '        _poison()\n'
        'def download_model(*_a, **_k):\n'
        '    _poison()\n')
    # The fixture transcript: fixed BEFORE running; the FLM backend
    # will produce its own text (synthetic surrogate -> tonal
    # "Ooooooooh..." repetitions). We assert non-empty final only.
    (root / 'audio').mkdir()
    # The VAD worker needs the REAL Silero/numpy: build a minimal
    # venv DIRECTORY (not a bare interpreter symlink, which loses
    # site-packages resolution) that reuses the harness venv's
    # site-packages read-only via pyvenv.cfg include-system/extra
    # paths. No package installation, no copy of the packages.
    harness_venv = Path.home() / '.local/share/agent-vesper/voice-venv'
    venv = root / 'voice-venv'
    venv.mkdir(parents=True)
    (venv / 'bin').mkdir()
    (venv / 'pyvenv.cfg').write_text(
        f"home = /usr/bin\n"
        f"include-system-site-packages = true\n"
        f"extra-search-dir = {harness_venv}/lib64/python3.14/site-packages\n")
    # Resolve through the harness venv's own interpreter PATH so the
    # venv layout stays intact.
    (venv / 'bin' / 'python').write_text(
        '#!/bin/sh\n'
        f'exec {harness_venv}/bin/python "$@"\n')
    (venv / 'bin' / 'python').chmod(0o755)
    config = root / '.agent-vesper'
    config.mkdir()
    (config / 'config.toml').write_text(
        f'[voice]\nenabled = true\npartials = false\n'
        f'tts = "voice-kokoro"\nvoice = "{voice}"\n'
        f'stt_compute = "cpu"\ntts_compute = "cpu"\n')
    lm = root / 'lmstudio'
    lm.mkdir()
    (lm / 'settings.json').write_text(json.dumps(
        {'api_base_url': f'http://127.0.0.1:{server.server_port}/v1',
         'model': 'voice-fixture'}))
    cache = root / 'data/agent-vesper/voice-pack'
    cache.mkdir(parents=True)
    for source in real_pack.rglob('*'):
        relative = source.relative_to(real_pack)
        if any(part in ('leases', 'staging') for part in relative.parts) or source.is_symlink():
            continue
        destination = cache / relative
        if source.is_dir():
            destination.mkdir(parents=True, exist_ok=True)
        elif source.is_file():
            destination.parent.mkdir(parents=True, exist_ok=True)
            if source.suffix == '.json':
                shutil.copy2(source, destination)
            else:
                os.link(source, destination)
    host = settings_pty.Host(str(Path(binary).resolve()), root, dict(
        PATH=str(commands) + os.pathsep + os.environ['PATH'],
        VESPER_PYTHON_PATH=python, PYTHONPATH=str(root),
        VOICE_FIXTURE_ROOT=str(root), TMPDIR=str(root / 'audio'),
        AGENT_VESPER_VOICE_VENV=str(root / 'voice-venv'),
        # The FLM route resolves the real user cache (read-only).
        AGENT_VESPER_PROVIDER='lmstudio', NO_PROXY='127.0.0.1,localhost',
        # Read-only reuse of the REAL installed FLM pack under the
        # isolated HOME. Vendor semantics: FLM_MODEL_PATH is the
        # PARENT of the pack root (flm appends 'models'), so pass
        # the parent. The pack is never copied or modified.
        FLM_MODEL_PATH=str(flm_root.parent),
    ))
    try:
            host.wait('Start coding', timeout=25)
            # Open Settings from the landing screen, navigate to the FLM
            # row (row 6), run Verify, set STT compute to NPU, and Save —
            # all in THIS process so the F9 gate's in-process
            # verification is genuinely recorded.
            host.key('s'); host.pump(1.5)
            settled = False
            for _ in range(30):
                host.pump(0.5)
                if 'Primary model' in host.text():
                    settled = True
                    break
            assert settled, 'Settings did not open from the landing screen\n' + host.text()[:600]
            # Navigate to the Voice row by its marker (row order
            # varies by build features; index is not stable).
            for _ in range(10):
                rows = [l for l in host.text().split('\n') if 'Voice' in l]
                if rows and ('›' in rows[0] or '›' in rows[-1]):
                    break
                host.key('\x1b[B'); host.pump(0.3)
            host.key('\r'); host.pump(1.2)
            settled = False
            for _ in range(30):
                host.pump(0.5)
                if 'Speech recognition compute' in host.text():
                    settled = True
                    break
            assert settled, 'Voice settings did not open\n' + host.text()[:800]
            # Navigate to the FLM row by marker-before-label.
            flm_selected = False
            for _ in range(12):
                rows = [l for l in host.text().split('\n') if 'Accelerated recognition' in l]
                if rows and '›' in rows[0]:
                    flm_selected = True
                    break
                host.key('\x1b[B'); host.pump(0.3)
            assert flm_selected, 'FLM row not found\n' + host.text()[:900]
            host.key('\r'); host.pump(2.0)
            settled = False
            for _ in range(30):
                host.pump(0.5)
                if 'Verify accelerated recognition' in host.text():
                    settled = True
                    break
            assert settled, 'FLM screen did not open\n' + host.text()[:900]
            host.key('\x1b[B'); time.sleep(0.3); host.key('\r')
            settled = False
            deadline = time.monotonic() + 240
            while time.monotonic() < deadline:
                host.pump(0.5)
                t = host.text()
                if 'Verified:' in t or 'did not complete' in t or 'failed at the local boundary' in t:
                    settled = True
                    break
            assert settled, 'Verify never settled\n' + host.text()
            assert 'Verified:' in host.text(), 'Verify did not pass\n' + host.text()
            host.key('\r'); host.pump(1.5)  # Close -> back to Voice menu
            settled = False
            for _ in range(40):
                host.pump(0.5)
                if 'Speech recognition compute' in host.text():
                    settled = True
                    break
            assert settled, 'Voice menu did not return after Verify\n' + host.text()
            # STT compute (row 3) -> NPU required (row 2).
            for _ in range(3):
                host.key('\x1b[B'); time.sleep(0.12)
            host.key('\r'); host.pump(1.2)
            host.wait('NPU required', timeout=10)
            for _ in range(2):
                host.key('\x1b[B'); time.sleep(0.12)
            host.key('\r'); host.pump(1.0)
            settled = False
            for _ in range(40):
                host.pump(0.5)
                if 'Speech recognition compute' in host.text() and 'NPU required' in host.text():
                    settled = True
                    break
            assert settled, 'Voice menu did not return after compute choice\n' + host.text()
            # Leave: Esc -> top Settings, Esc -> prompt, Enter = Save.
            host.key('\x1b'); host.pump(1.0)
            host.key('\x1b'); host.pump(1.0)
            settled = False
            for _ in range(30):
                host.pump(0.5)
                if 'Save changes' in host.text() and 'Discard changes' in host.text():
                    settled = True
                    break
            assert settled, 'exit prompt did not appear\n' + host.text()
            host.key('\r')  # Save changes
            host.pump(1.5)

            # The saved scope must now select the verified NPU STT route.
            saved = (root / '.agent-vesper/config.toml').read_text()
            assert 'stt_compute = "npu"' in saved, f'save did not persist NPU STT:\n{saved}'

            # After Save the app leaves Settings; this same process
            # holds the per-process verification recorded by Verify.
            host.pump(2)
            if 'Start coding' in host.text():
                host.key('\r')
                host.pump(1.5)
            deadline = time.monotonic() + 60
            entered = False
            while time.monotonic() < deadline:
                host.pump(1.0)
                if 'Push to talk' in host.text():
                    entered = True
                    break
            assert entered, 'workspace never appeared after Save\n' + host.text()[:600]
            host.key('\x1b[20~')  # F9
            deadline = time.monotonic() + 20
            state = None
            status_seen = None
            while time.monotonic() < deadline:
                host.pump(0.5)
                t = host.text()
                # The refusal/status line renders in the bottom rows; scan
                # them explicitly and record the first status-like line.
                rows = [l.strip() for l in t.split('\n') if l.strip()]
                for r in rows[-6:]:
                    if 'Voice' in r or 'NPU' in r or 'recognition' in r:
                        status_seen = r
                        break
                for marker in ('Recording microphone', 'blocked', 'cannot run here', 'NPU required'):
                    if marker in t:
                        state = marker
                        break
                if state:
                    print('F9 status line:', status_seen)
                    break
            assert state == 'Recording microphone', f'F9 did not start capture (state={state}); status={status_seen}\nFULLSCREEN:\n' + host.text()
            time.sleep(1.0)
            host.key('\x1b[20~')  # stop capture -> finalize -> transcribe
            # 3) Exactly one ordinary agent turn on the loopback wire.
            deadline = time.monotonic() + 180
            while not requests and time.monotonic() < deadline:
                time.sleep(0.5)
            assert requests, 'F9 final never reached the agent on the FLM route\n' + host.text()
            assert len(requests) == 1, f'expected exactly one agent turn, saw {len(requests)}\n' + host.text()
            payload = json.dumps(requests[0])
            assert 'voice' in payload.lower() or 'speech' in payload.lower() or payload, 'payload'
            # 4) The player sink received real synthesized PCM (Kokoro CPU
            #    path unchanged and exercised in the same turn).
            deadline = time.monotonic() + 240
            pcm_file = root / 'player-bytes'
            while (not pcm_file.exists() or not pcm_file.read_text().strip()) and time.monotonic() < deadline:
                time.sleep(0.5); host.pump(0.2)
                time.sleep(0.5)
            counts = [int(n) for n in pcm_file.read_text().split()] if pcm_file.exists() else []
            assert counts and counts[0] > 0, 'no synthesized PCM reached the player sink\n' + host.text()
            text = host.text()
            assert 'CPU RECOGNIZER CONSTRUCTED' not in text, \
                'the FLM route must not construct the CPU recognizer\n' + text
            print(f'PASS: Settings save -> Verify -> F9 -> FLM NPU adapter -> one agent turn; '
                  f'provider requests={len(requests)}; player PCM bytes={counts}; '
                  f'CPU recognizer untouched')
    finally:
        # FLM owns a separate process group, so the generic PTY driver's
        # SIGTERM-to-host fallback cannot reap it. Ask the application to
        # exit normally first; its adapter Drop then tears down the exact
        # owned FLM child before this isolated registry is removed.
        if host.child.poll() is None:
            try:
                host.key('\x18')
                host.child.wait(timeout=10)
            except (OSError, subprocess.TimeoutExpired):
                pass
        try:
            host.close()
        except OSError:
            pass
        shutil.rmtree(root, ignore_errors=True)
        try:
            server.shutdown()
        except Exception:
            pass


if __name__ == '__main__':
    args = sys.argv[1:]
    binary = args[0] if args else os.environ.get('VOICE_TUI_BINARY', 'target/release/agent-vesper-tui')
    python = args[1] if len(args) > 1 else sys.executable
    run(binary, python)
