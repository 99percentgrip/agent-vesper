"""Controlled inference stand-in; production PCM slicing/protocol remain real."""
import os
from pathlib import Path
import time
from types import SimpleNamespace

class WhisperModel:
    def __init__(self, *args, **kwargs):
        self.root = Path(os.environ['VOICE_FIXTURE_ROOT'])
        with (self.root/'model-loads').open('a') as f:
            f.write('loaded\n')
        self.index = 0
    def transcribe(self, samples, **kwargs):
        # VAD contract (silence-hallucination PRD): the sidecar must pass
        # vad_filter=True on every call; the fixture records it so the PTY
        # suite can assert the production script actually sends it.
        assert len(samples) == 30*16000
        if kwargs.get('vad_filter') is True:
            with (self.root/'vad').open('a') as f:
                f.write('vad\n')
        self.index += 1
        if (self.root/'fail').exists() and self.index == 2:
            raise RuntimeError('fixture failure')
        if (self.root/'delay').exists():
            time.sleep(float((self.root/'delay').read_text()))
        return iter([SimpleNamespace(text='dictation')]), None
