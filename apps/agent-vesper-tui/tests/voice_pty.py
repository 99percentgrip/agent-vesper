#!/usr/bin/env python3
"""Real terminal voice lifecycle using isolated, microphone-free helpers.

Requires a test Python with numpy (passed as second argument); never bootstraps
packages, calls a provider, or reads audio from the user's devices.
"""
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
from settings_pty import Host


def click_footer(host, label):
    host.wait(label)
    line = ''.join(host.screen[-1])
    x = line.index(label) + 1
    host.key(f'\x1b[<0;{x};40M')


def run(binary, python):
    fixture = Path(__file__).parent
    with tempfile.TemporaryDirectory(prefix='vesper-voice-pty-') as temp:
        root = Path(temp)
        commands = root/'bin'
        commands.mkdir()
        source = (fixture/'voice_recorder_fixture.py').read_text().split('\n',1)[1]
        for name in ['arecord','afrecord']:
            p = commands/name
            p.write_text('#!'+python+'\n'+source)
            p.chmod(0o755)
        (root/'faster_whisper.py').write_text((fixture/'voice_model_fixture.py').read_text())
        audio = root/'audio'
        audio.mkdir()
        host = Host(binary, root, dict(PATH=str(commands)+os.pathsep+os.environ['PATH'],
                    VESPER_PYTHON_PATH=python, PYTHONPATH=str(root),
                    VOICE_FIXTURE_ROOT=str(root), TMPDIR=str(audio)))
        try:
            host.wait('Start coding')
            host.key('\r')
            host.wait('Push to talk')
            host.key('preserved ')
            click_footer(host, 'Push to talk')
            host.wait('Recording microphone')
            host.wait('Stop')
            host.pump(1.2)
            assert '00:01' in host.text(), host.text()
            (root/'fail').touch()
            click_footer(host, 'Stop')
            host.wait('Retry voice')
            assert list(audio.glob('vesper-voice-*/recording.wav'))
            (root/'fail').unlink()
            click_footer(host, 'Retry voice')
            host.wait('Push to talk')
            assert 'preserved dictation' in '\n'.join(''.join(row) for row in host.screen[-3:-1]), host.text()
            assert not list(audio.glob('vesper-voice-*'))
            host.key('\x7f'*250)
            loads = (root/'model-loads').read_text().count('loaded')
            (root/'vad').unlink(missing_ok=True)
            # More than 90 seconds of real wall-clock transcription with progress.
            (root/'delay').write_text('5')
            host.key('\x1b[15~')
            host.wait('Recording microphone')
            host.key('\x1b[15~')
            started = time.monotonic()
            host.wait('Transcribing')
            host.key(' editable')
            assert 'editable' in host.text(), host.text()
            host.wait('Push to talk', timeout=125)
            assert time.monotonic()-started > 90
            assert (root/'model-loads').read_text().count('loaded') == loads
            # VAD contract: every production transcribe call must pass
            # vad_filter=True (silence-hallucination guard; the fixture
            # records each filtered call).
            assert (root/'vad').exists() and (root/'vad').read_text().count('vad') >= 2, \
                'production sidecar must pass vad_filter=True'
            (root/'delay').unlink()
            # F5 remains reachable through command-menu and focus-mode interaction.
            host.key('\x7f'*500)
            host.key('\x1b[15~'); host.wait('Recording microphone')
            host.key('\x1b[23~')
            host.key('/')
            host.wait('Complete')
            host.key('\x1b[15~'); host.wait('Push to talk')
            host.key('\x7f'*250)
            host.key('\x1b[23~')
            # Cancellation retains audio; discard removes it.
            (root/'delay').write_text('5')
            host.key('\x1b[15~'); host.wait('Recording microphone')
            host.key('\x1b[15~'); host.wait('Transcribing')
            host.key('\x1b[15~'); host.wait('Retry voice')
            click_footer(host,'Discard'); host.wait('Push to talk')
            assert not list(audio.glob('vesper-voice-*'))
            (root/'delay').unlink()
            (root/'early-exit').touch()
            host.key('\x1b[15~'); host.wait('stopped unexpectedly')
            host.key('\x1b[3~'); host.wait('Push to talk')
            (root/'early-exit').unlink()
            (root/'disk-failure').touch()
            host.key('\x1b[15~'); host.wait('stopped unexpectedly')
            host.key('\x1b[15~'); host.wait('No recoverable audio')
            host.key('\x1b[3~'); host.wait('Push to talk')
            (root/'disk-failure').unlink()
            # Normal exit while recording must reap recorder and remove audio.
            host.key('\x1b[15~'); host.wait('Recording microphone')
            host.key('\x18')
            host.child.wait(timeout=10)
            assert not list(audio.glob('vesper-voice-*'))
            print('PASS: mouse/F5, 10-minute PCM, >90-second progressing transcription, editable composer, retry/discard, early exit, disk failure and shutdown cleanup')
        finally:
            host.close()
            pid_file = root/'recorder-pid'
            if pid_file.exists():
                pid = int(pid_file.read_text())
                current = subprocess.run(['ps','-p',str(pid),'-o','args='],capture_output=True,text=True).stdout
                if str(commands) in current:
                    try: os.kill(pid,signal.SIGKILL)
                    except ProcessLookupError: pass

if __name__ == '__main__':
    run(str(Path(sys.argv[1]).resolve()),str(Path(sys.argv[2]).absolute()))
