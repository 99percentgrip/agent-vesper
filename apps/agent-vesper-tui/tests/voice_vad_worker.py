#!/usr/bin/env python3
"""Device-free contract test for the persistent shared Silero VAD worker."""
from __future__ import annotations

import json
import os
import struct
import subprocess
import sys
import tempfile
import wave
from pathlib import Path


def wav(path: Path, samples: list[int]) -> None:
    with wave.open(str(path), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(16000)
        out.writeframes(struct.pack(f"<{len(samples)}h", *samples))


def receive(proc: subprocess.Popen[str]) -> dict:
    line = proc.stdout.readline()
    if not line:
        raise AssertionError(f"worker exited early: {proc.stderr.read()}")
    return json.loads(line)


def main() -> None:
    worker = Path(__file__).parents[1] / "src" / "voice_vad.py"
    with tempfile.TemporaryDirectory(prefix="vesper-vad-") as directory:
        root = Path(directory)
        package = root / "faster_whisper"
        package.mkdir()
        (package / "__init__.py").write_text("")
        (package / "vad.py").write_text(
            """loads = 0
class VadOptions: pass
def get_speech_timestamps(samples, options):
    global loads
    loads += 1
    return [] if (len(samples) == 0 or float(abs(samples).max()) == 0) else [{'start': 1, 'end': len(samples)-1}]
def collect_chunks(samples, chunks):
    c = chunks[0]
    return samples[c['start']:c['end']]
"""
        )
        silent, speech, filtered = root / "silent.wav", root / "speech.wav", root / "filtered.wav"
        wav(silent, [0] * 160)
        wav(speech, [0, 12000, -12000, 0])
        env = dict(os.environ)
        env["PYTHONPATH"] = str(root)
        python = Path(os.environ.get("AGENT_VESPER_VOICE_PYTHON", sys.executable))
        proc = subprocess.Popen(
            [str(python), str(worker)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, env=env,
        )
        assert receive(proc) == {"ready": True, "protocol": 1}
        for request in [
            {"op": "filter", "id": 1, "input_wav": str(silent), "output_wav": str(filtered)},
            {"op": "filter", "id": 2, "input_wav": str(speech), "output_wav": str(filtered)},
        ]:
            proc.stdin.write(json.dumps(request) + "\n")
            proc.stdin.flush()
            result = receive(proc)
            assert result["id"] == request["id"]
            if request["id"] == 1:
                assert result["speech"] is False and not filtered.exists()
            else:
                assert result["speech"] is True and result["output_samples"] == 2
                with wave.open(str(filtered), "rb") as got:
                    assert (got.getnchannels(), got.getsampwidth(), got.getframerate()) == (1, 2, 16000)
        proc.stdin.write('{"op":"filter","id":3,"input_wav":"missing","output_wav":"secret"}\n')
        proc.stdin.flush()
        assert receive(proc) == {"id": 3, "error": "VAD failed"}
        proc.stdin.write('{"op":"shutdown"}\n')
        proc.stdin.flush()
        assert receive(proc) == {"shutdown": True}
        assert proc.wait(timeout=5) == 0
    print("PASS: persistent VAD protocol, silence provenance, canonical output, redacted failure")


if __name__ == "__main__":
    main()
