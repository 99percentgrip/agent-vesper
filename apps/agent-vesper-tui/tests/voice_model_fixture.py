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
    def transcribe(self, samples):
        assert len(samples) == 30*16000
        self.index += 1
        if (self.root/'fail').exists() and self.index == 2:
            raise RuntimeError('fixture failure')
        if (self.root/'delay').exists():
            time.sleep(float((self.root/'delay').read_text()))
        return iter([SimpleNamespace(text='dictation')]), None
