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
# R20 repair: the production Linux recorder now streams WAV on stdout
# (`arecord ... -`), drained through the managed capture's capped writer.
# Mirror real arecord semantics: `-` means stdout.
_dest = sys.argv[-1]
_streaming = (_dest == '-')
_sink = sys.stdout.buffer if _streaming else None
# R20: real `arecord ... -` streams a WAV header once + raw frames (no
# re-seeking on a pipe). Mirror that exactly; the managed capture's
# writer patches the final sizes (its own finalize step).
import struct as _struct
import time as _t
def _emit(chunk):
    if _streaming:
        _sink.write(chunk); _sink.flush()
    else:
        with open(_dest, 'ab') as _f:
            _f.write(chunk)
if _streaming:
    _header = b'RIFF' + _struct.pack('<I', 0xFFFFFFFF) + b'WAVEfmt ' + _struct.pack(
        '<IHHIIHH', 16, 1, 1, 16000, 32000, 2, 16) + b'data' + _struct.pack('<I', 0xFFFFFFFF)
    _emit(_header)
else:
    with wave.open(_dest, 'wb') as wav:
        wav.setnchannels(1); wav.setsampwidth(2); wav.setframerate(16000)
        for _ in range(20):
            wav.writeframes(b'\0\0' * (30*16000))
    _sink = object()  # marker; loop below skipped for file mode
if _streaming:
    if (root/'early-exit').exists() or (root/'disk-failure').exists():
        # Failure probes: die before/without a capped stream so the
        # recorder's abnormal exit is observable (a long stream would
        # hit the store's byte cap first and end the capture normally).
        if (root/'disk-failure').exists():
            sys.exit(1)
        _emit(b'\0\0' * 16000)  # a little real audio, then die
        sys.exit(1)
    # Fast-but-bounded pace (~60 s of frames per wall second): a ~2 s
    # capture holds >60 s of audio (2+ sidecar chunks for the 'fail'
    # test) and stays far under the 120 s/4 MiB hard caps.
    for _ in range(60):
        _emit(b'\0\0' * (60*16000))
        _t.sleep(1.0)
if (root/'early-exit').exists():
    sys.exit(1)
signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
while True:
    time.sleep(.1)
