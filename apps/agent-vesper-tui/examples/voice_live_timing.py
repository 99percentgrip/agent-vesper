#!/usr/bin/env python3
"""Explicit single native live-provider turn; fixture capture/STT/player, real Kokoro.

Requires separate approval and --execute-live. Uses saved provider/model/reasoning
choices and the provider's normal credential store without printing credentials.
No microphone, speaker, installed-binary replacement, or model-selected tools.
Mutable session/UI/memory state is confined to a temporary workspace. This is a
fresh-turn diagnostic, not a replay of existing history or real transcription.
"""
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import time

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / 'apps/agent-vesper-tui/tests'))
import settings_pty


def main():
    if sys.argv[1:] != ['--execute-live']:
        print('No live execution. Requires explicit approval and --execute-live.')
        return
    home = Path.home()
    provider = (REPO / '.agent-vesper/provider').read_text().strip()
    # This diagnostic's credential-path isolation is implemented for the active
    # OpenAI adapter only; do not silently select another provider or credential.
    if provider != 'openai':
        raise SystemExit('This launcher requires the already-selected OpenAI adapter.')
    credentials = Path(os.environ.get('AGENT_VESPER_OPENAI_CREDENTIALS_PATH',
        str(Path(os.environ.get('XDG_CONFIG_HOME', str(home / '.config'))) / 'agent-vesper/openai-credentials.json')))
    python = home / '.local/share/agent-vesper/voice-venv/bin/python'
    if not python.is_file():
        raise SystemExit('Existing local transcription Python required; no setup attempted.')
    real_pack = home / '.local/share/agent-vesper/voice-pack'
    saved = json.loads((home / '.agent-vesper/ui/settings.json').read_text())
    print('Saved execution choices:', json.dumps(saved['providers'][provider], sort_keys=True))
    fixture = REPO / 'apps/agent-vesper-tui/tests'
    with tempfile.TemporaryDirectory(prefix='.voice-live-', dir=REPO) as tmp:
        root = Path(tmp)
        commands = root / 'bin'
        commands.mkdir()
        (root / 'home').mkdir()
        ui = root / 'home/.agent-vesper/ui'
        ui.mkdir(parents=True)
        # Preserve model/reasoning; narrow only permissions in this isolated copy.
        saved.setdefault('common', {})['/permission'] = '/permission read'
        (ui / 'settings.json').write_text(json.dumps(saved))
        shutil.copy2(REPO / 'AGENTS.md', root / 'AGENTS.md')
        config = root / '.agent-vesper'
        config.mkdir()
        for name in ('provider', 'config.toml', 'skill-routing.json'):
            source = REPO / '.agent-vesper' / name
            if source.is_file():
                shutil.copy2(source, config / name)
        # Preserve available routing metadata without letting a diagnostic write
        # the user's library. Cognition/session history starts empty, explicitly.
        for source, dest in [(home / '.agent-vesper/memory', root / 'global-memory'),
                             (REPO / '.agent-vesper/memory', config / 'memory')]:
            if source.is_dir():
                shutil.copytree(source, dest)
        for name in ('audio', 'voice-venv/bin'):
            (root / name).mkdir(parents=True)
        (root / 'voice-venv/bin/python').symlink_to(python)
        (root / 'faster_whisper.py').write_text((fixture / 'voice_model_fixture.py').read_text())
        (root / 'transcript').write_text('In one sentence, explain why a Rust mutex guard should not be held across blocking audio playback. Do not use tools or files.')
        recorder = commands / 'arecord'
        recorder.write_text('#!/usr/bin/env python3\n' + (fixture / 'voice_recorder_fixture.py').read_text().split('\n', 1)[1])
        recorder.chmod(0o755)
        player = commands / 'aplay'
        player.write_text('''#!/usr/bin/env python3
import os,sys,time
from pathlib import Path
root=Path(os.environ['VOICE_FIXTURE_ROOT'])
first=sys.stdin.buffer.read(2)
with (root/'player-first-pcm').open('a') as out: out.write(str(time.monotonic())+'\\n')
data=first+sys.stdin.buffer.read()
with (root/'player-bytes').open('a') as out: out.write(str(len(data))+'\\n')
''')
        player.chmod(0o755)
        cache = root / 'data/agent-vesper/voice-pack'
        cache.mkdir(parents=True)
        for source in real_pack.rglob('*'):
            relative = source.relative_to(real_pack)
            if any(part in ('leases', 'staging') for part in relative.parts) or source.is_symlink():
                continue
            dest = cache / relative
            if source.is_dir(): dest.mkdir(parents=True, exist_ok=True)
            elif source.is_file():
                dest.parent.mkdir(parents=True, exist_ok=True)
                if source.suffix == '.json': shutil.copy2(source, dest)
                else: os.link(source, dest)
        env = dict(os.environ)
        env.update(HOME=str(root/'home'), USERPROFILE=str(root/'home'),
            XDG_DATA_HOME=str(root/'data'), XDG_CONFIG_HOME=str(root/'config'),
            AGENT_VESPER_HOME=str(config), AGENT_VESPER_PROVIDER=provider,
            AGENT_VESPER_OPENAI_CREDENTIALS_PATH=str(credentials),
            AGENT_VESPER_GLOBAL_MEMORY_ROOT=str(root/'global-memory'),
            AGENT_VESPER_GLOBAL_COGNITION_ROOT=str(root/'global-cognition'),
            AGENT_VESPER_COGNITION_ROOT=str(root/'cognition'),
            AGENT_VESPER_CHECKPOINT_ROOT=str(root/'checkpoints'),
            AGENT_VESPER_SESSION_ROOT=str(root/'sessions'),
            PATH=str(commands)+os.pathsep+os.environ['PATH'],
            VESPER_PYTHON_PATH=str(python), PYTHONPATH=str(root),
            VOICE_FIXTURE_ROOT=str(root), TMPDIR=str(root/'audio'),
            AGENT_VESPER_VOICE_VENV=str(root/'voice-venv'))
        # Host's default outbound blockade belongs to fixture tests, not this
        # separately authorized live example. Retain actual configured proxies.
        for key in ('HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'NO_PROXY'):
            env[key] = os.environ.get(key, '')
        host = settings_pty.Host(str(REPO/'target/debug/agent-vesper-tui'), root, env)
        try:
            host.wait('Start coding', timeout=35)
            host.key('\r')
            host.wait('Push to talk')
            host.pump(2)
            host.key('\x1b[20~')
            host.wait('Recording microphone', timeout=8)  # fixture only
            host.pump(1)
            start = time.monotonic()
            host.key('\x1b[20~')
            observed = {}
            while time.monotonic()-start < 90:
                host.pump(0.05)
                text = host.text()
                for label in ('Transcribing', 'waiting for speakable agent text',
                              'Synthesizing voice', 'Sending PCM to player', 'Waiting for player drain'):
                    if label in text and label not in observed:
                        observed[label] = round(time.monotonic()-start, 3)
                        print(f't={observed[label]:.3f}s {label}', flush=True)
                # A tool is not part of this approved probe. Do not grant any
                # approval or continue an unexpected tool trajectory.
                if 'Approve' in text and 'Deny' in text:
                    raise RuntimeError('unexpected tool approval; cancelling probe')
                if (root/'player-bytes').exists():
                    first = float((root/'player-first-pcm').read_text().splitlines()[0])
                    counts = [int(n) for n in (root/'player-bytes').read_text().splitlines()]
                    print(json.dumps({'stop_to_first_pcm_s': round(first-start, 3),
                        'observed_stage_s': observed, 'pcm_bytes': counts,
                        'scope': 'one fresh native live turn; fixture capture/STT/player; real selected provider and Kokoro; no acoustic timing'}, sort_keys=True))
                    return
                if any(marker in text for marker in ('speech failed:', 'Authentication required', 'Turn interrupted')):
                    raise RuntimeError('native turn failed; no automatic retry')
            raise RuntimeError('90-second turn deadline; no automatic retry')
        except Exception as error:
            # Host.wait exceptions contain screens; never forward them to logs.
            print('Live timing did not complete:', type(error).__name__, flush=True)
            host.key('\x03')
            raise SystemExit(2) from None
        finally:
            host.close()


if __name__ == '__main__':
    main()
