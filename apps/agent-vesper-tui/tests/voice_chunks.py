#!/usr/bin/env python3
"""Ten-minute PCM ordering/resume check using production chunk slicing and real numpy."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import struct
import tempfile
from types import SimpleNamespace
import wave

source = Path(__file__).parents[1]/'src/voice_transcribe.py'
spec = importlib.util.spec_from_file_location('voice_transcribe', source)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
class Model:
    def transcribe(self, samples):
        assert len(samples) == 480000
        marker = round(float(samples[0])*32768)
        return [SimpleNamespace(text=f'word{marker}')], None
with tempfile.TemporaryDirectory() as temp:
    path = Path(temp)/'long.wav'
    with wave.open(str(path),'wb') as audio:
        audio.setnchannels(1); audio.setsampwidth(2); audio.setframerate(16000)
        for marker in range(1,21):
            audio.writeframes(struct.pack('<h',marker)*480000)
    for skip in [0,5,20]:
        output=io.StringIO()
        with contextlib.redirect_stdout(output): module.transcribe(str(path),skip,Model())
        rows=[json.loads(line) for line in output.getvalue().splitlines()]
        assert [row['text'] for row in rows[:-1]] == [f'word{i}' for i in range(skip+1,21)]
        assert [row['index'] for row in rows[:-1]] == list(range(skip,20))
        assert rows[-1] == {'done':True,'chunks':20}
    print('PASS ten-minute PCM, bounded slices, exact ordering, partial retry and completed retry')
