#!/usr/bin/env python3
"""Deterministic recorder fixture: never opens a microphone."""
import os
from pathlib import Path
import signal
import sys
import time
import wave
root = Path(os.environ['VOICE_FIXTURE_ROOT'])
(root/'recorder-pid').write_text(str(os.getpid()))
if (root/'disk-failure').exists():
    sys.exit(1)
with wave.open(sys.argv[-1], 'wb') as wav:
    wav.setnchannels(1)
    wav.setsampwidth(2)
    wav.setframerate(16000)
    for _ in range(20):
        wav.writeframes(b'\0\0' * (30*16000))
if (root/'early-exit').exists():
    sys.exit(1)
signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
while True:
    time.sleep(.1)
