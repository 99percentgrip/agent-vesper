#!/usr/bin/env python3
"""Native F9 regression: loopback provider, fixture capture/STT/player, real Kokoro.

No injected AgentEvents, public provider, microphone, speaker, downloads, or
user-state writes. Existing verified assets are hard-linked read-only into an
isolated same-filesystem cache; metadata is copied, not shared. This is reuse
verification, not a fresh installation or listening acceptance.
"""
import http.server
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import threading
import time

sys.path.insert(0, str(Path(__file__).parent))
import settings_pty


def run(binary, python, voice="am_michael", mode="off"):
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
            payload = json.dumps(request)
            # Test actual provider-visible instructions, not an embedded phrase.
            if 'Native voice conversation' not in payload:
                text = 'Missing native voice contract.'
            elif mode == 'list':
                # VRO-17 formatting/continuity repair: a realistic streamed
                # numbered answer (word-sized deltas like a real provider).
                text = ('Here is the plan. 1. First we measure the pipeline with a paced sink. '
                        '2. Then we repair the smallest owning boundary. 3. Finally we verify through the production path tests.')
            elif mode == 'continuity':
                # Normalized reconstruction of Alex's demonstrated long reply.
                # Synthetic fixture text: not asserted as a verbatim transcript.
                text = ("I'm running inside the Agent Vesper harness with cognitive memory active, so context from past sessions carries over automatically. "
                        'The workspace is intact, and nothing is blocking me right now: no pending failures or half-finished work on my side. '
                        'Tools, skills, and the project contracts are all loaded and ready for whatever you want to do next. '
                        "I'm only waiting on you to confirm this came through audibly.")
            else:
                text = 'Understood. The native voice reply is routed by the host.'
            if mode in ('list', 'continuity'):
                # Stream in word-sized SSE deltas. The continuity fixture
                # deliberately splits `automatically` at a transport boundary;
                # this must not become a speech/inference boundary.
                chunks = []
                words = text.split(' ')
                carried = ''
                for word in words:
                    if mode == 'continuity' and word.startswith('automatically'):
                        if carried:
                            chunks.append(carried)
                            carried = ''
                        chunks.extend(['automatic', word[len('automatic'):] + ' '])
                        continue
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
            else:
                data = ('data: ' + json.dumps({'choices': [{'index': 0, 'delta': {'content': text}, 'finish_reason': None}]}) + '\n\n'
                    + 'data: ' + json.dumps({'choices': [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}]}) + '\n\ndata: [DONE]\n\n').encode()
            # Deliberate provider hold proves upstream wait is visible in the
            # actual frame, not merely stored in SessionState.
            time.sleep(1)
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
    # Same filesystem is required for hard links: no duplicate large assets.
    try:
        with tempfile.TemporaryDirectory(prefix='.voice-loop-', dir=Path.cwd()) as temp:
            root = Path(temp)
            commands = root / 'bin'
            commands.mkdir()
            espeak = shutil.which('espeak-ng')
            assert espeak, 'verified pronunciation prerequisite required'
            phonemizer = commands / 'espeak-ng'
            phonemizer.write_text('#!/usr/bin/env python3\nimport os,sys\n'
                'if "--ipa" not in sys.argv or "-q" not in sys.argv or "--stdout" in sys.argv: sys.exit(91)\n'
                + f'os.execv({espeak!r}, [{espeak!r}] + sys.argv[1:])\n')
            phonemizer.chmod(0o755)
            recorder = commands / 'arecord'
            recorder.write_text('#!/usr/bin/env python3\n' + (fixture / 'voice_recorder_fixture.py').read_text().split('\n', 1)[1])
            recorder.chmod(0o755)
            player = commands / 'aplay'
            player.write_text('#!/usr/bin/env python3\nimport os,sys,struct,time\n'
                'first = sys.stdin.buffer.read(2)\n'
                'with open(os.path.join(os.environ["VOICE_FIXTURE_ROOT"], "player-first-pcm"), "a") as f: f.write(str(time.monotonic())+"\\n")\n'
                'data = first + sys.stdin.buffer.read()\nn = len(data)\n'
                'peak = max((abs(s[0]) for s in struct.iter_unpack("<h", data)), default=0)\n'
                'with open(os.path.join(os.environ["VOICE_FIXTURE_ROOT"], "player-peaks"), "a") as f: f.write(str(peak)+"\\n")\n'
                'with open(os.path.join(os.environ["VOICE_FIXTURE_ROOT"], "player-bytes"), "a") as f: f.write(str(n)+"\\n")\n')
            player.chmod(0o755)
            (root / 'faster_whisper.py').write_text('import time; time.sleep(2)\n' + (fixture / 'voice_model_fixture.py').read_text())
            (root / 'transcript').write_text('Please reply by voice.' if mode == 'off' else 'Solve this differential equation for the general case and reply by voice.')
            (root / 'audio').mkdir()
            venv = root / 'voice-venv/bin'
            venv.mkdir(parents=True)
            (venv / 'python').symlink_to(python)
            config = root / '.agent-vesper'
            config.mkdir()
            (config / 'config.toml').write_text(f'[voice]\nenabled = true\npartials = false\ntts = "voice-kokoro"\nvoice = "{voice}"\n')
            lm = root / 'lmstudio'
            lm.mkdir()
            (lm / 'settings.json').write_text(json.dumps({'api_base_url': f'http://127.0.0.1:{server.server_port}/v1', 'model': 'voice-fixture'}))
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
                VESPER_PYTHON_PATH=python, PYTHONPATH=str(root), VOICE_FIXTURE_ROOT=str(root),
                TMPDIR=str(root / 'audio'), AGENT_VESPER_VOICE_VENV=str(root / 'voice-venv'),
                AGENT_VESPER_PROVIDER='lmstudio', NO_PROXY='127.0.0.1,localhost',
            ))
            try:
                host.wait('Start coding', timeout=25)
                if mode == 'preview':
                    host.key('s')
                    host.click('Voice')
                    host.click('Natural Voice pack')
                    host.wait('Preview voice')
                    preview_start = time.monotonic()
                    host.click('Preview voice')
                    deadline = time.monotonic() + 30
                    saw_synthesis = False
                    while time.monotonic() < deadline and not (root / 'player-bytes').exists():
                        host.pump(0.05)
                        saw_synthesis |= 'Synthesizing voice' in host.text()
                    assert saw_synthesis, 'Preview did not expose the actual synthesis stage'
                    assert (root / 'player-bytes').exists(), 'Preview produced no PCM\n' + host.text()
                    first = float((root / 'player-first-pcm').read_text().splitlines()[0])
                    peak = int((root / 'player-peaks').read_text().splitlines()[0])
                    assert peak >= 256, f'Preview near-zero PCM: {peak}'
                    assert not requests, 'Preview submitted a provider request'
                    assert not (root / 'recorder-pid').exists(), 'Preview started the recorder'
                    print(f'PASS: actual Settings Preview -> real Kokoro -> fake player; click-to-first-PCM={(first-preview_start)*1000:.1f} ms; peak={peak}; no recorder/provider')
                    return
                host.key('\r')
                host.wait('Push to talk')
                reasoning_mode = mode if mode in ('off', 'balanced') else 'off'
                host.key('/reasoning set mode=' + reasoning_mode + '\r')
                # Allow background integrity preflight; recorder onset must not
                # wait for our deliberately slow (2 s) Python import.
                host.pump(2)
                capture_start = time.monotonic()
                host.key('\x1b[20~')
                while not (root / 'recorder-pid').exists() and time.monotonic() - capture_start < 5:
                    host.pump(0.025)
                capture_ms = (time.monotonic() - capture_start) * 1000
                assert (root / 'recorder-pid').exists() and capture_ms < 1000, f'recorder start blocked: {capture_ms:.1f} ms'
                host.wait('Recording microphone', timeout=5)
                host.pump(2)
                reply_start = time.monotonic()
                host.key('\x1b[20~')
                deadline = time.monotonic() + 90
                saw_agent_wait = False
                while time.monotonic() < deadline:
                    host.pump(0.25)
                    saw_agent_wait |= 'waiting for speakable agent text' in host.text()
                    if (root / 'player-bytes').exists() and len((root / 'player-bytes').read_text().splitlines()) >= 2:
                        break
                counts = [int(n) for n in (root / 'player-bytes').read_text().splitlines()] if (root / 'player-bytes').exists() else []
                assert saw_agent_wait, 'native running frame hid the upstream voice wait status\n' + host.text()
                assert requests, 'no request reached the loopback provider\n' + host.text()
                assert all('Native voice conversation' in json.dumps(r) for r in requests), 'F9 instruction missing on chat wire\n' + host.text()
                assert counts and all(n > 0 for n in counts), 'no Kokoro PCM reached the fixture player\n' + host.text()
                peaks = [int(n) for n in (root / 'player-peaks').read_text().splitlines()]
                assert len(peaks) == len(counts) and all(p >= 256 for p in peaks), f'near-zero Kokoro PCM: {peaks}'

                if mode == 'list':
                    assert 'Finally we verify' in host.text(), 'list answer text missing'
                    assert 'speech failed' not in host.text(), 'list answer hit the phoneme failure: '+repr([l for l in host.text().splitlines() if 'speech failed' in l][:3])
                elif mode == 'continuity':
                    assert 'carries over automatically' in host.text(), 'subword transport split changed displayed content'
                    assert 'came through audibly' in host.text(), 'long answer tail missing'
                    assert 'speech failed' not in host.text(), 'continuity fixture failed speech'
                else:
                    assert 'native voice reply' in host.text(), 'text answer missing'
                if mode not in ('off', 'list'):
                    assert 'VRO' in host.raw, 'fixture did not exercise VRO final-only route'
                assert 'speech failed:' not in host.text(), host.text()
                first_pcm_ms = (float((root / 'player-first-pcm').read_text().splitlines()[0]) - reply_start) * 1000
                print(f'LATENCY: recorder process observed={capture_ms:.1f} ms; stop-to-first-PCM={first_pcm_ms:.1f} ms; complete fixture reply={(time.monotonic()-reply_start)*1000:.1f} ms; includes PTY polling, synthetic STT/provider with 1 s hold, real Kokoro; not real end-to-end latency')
                print(f'PASS: F9 -> real agent/provider wire -> hygiene -> Kokoro {voice} ({mode}) -> fixture player; PCM bytes={counts}; peaks={peaks}; provider requests={len(requests)}')
                print('Device/listening acceptance NOT performed. No fresh install; existing asset inodes reused.')
            finally:
                host.close()
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)

if __name__ == '__main__':
    run(sys.argv[1] if len(sys.argv) > 1 else 'target/debug/agent-vesper-tui',
        sys.argv[2] if len(sys.argv) > 2 else str(Path.home() / '.local/share/agent-vesper/voice-venv/bin/python'),
        sys.argv[3] if len(sys.argv) > 3 else 'am_michael',
        sys.argv[4] if len(sys.argv) > 4 else 'off')
