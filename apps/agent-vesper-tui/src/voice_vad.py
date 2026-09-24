"""Persistent local Silero VAD worker for speech backends without their own VAD.

The worker is deliberately backend-neutral: it reads canonical 16 kHz mono/i16
WAV files, uses the already-installed faster-whisper Silero model, and writes a
speech-only WAV.  Stdout is a bounded line-delimited JSON protocol; diagnostics
never include audio or transcript content.
"""
from __future__ import annotations

import json
import os
import sys
import tempfile
import wave
from pathlib import Path

MAX_CAPTURE_BYTES = 64 * 1024 * 1024


def emit(value: dict) -> None:
    print(json.dumps(value, separators=(",", ":")), flush=True)


def _load_vad():
    from faster_whisper.vad import VadOptions, collect_chunks, get_speech_timestamps

    return VadOptions, collect_chunks, get_speech_timestamps


def filter_wav(source: str, destination: str) -> dict:
    import numpy as np

    source_path = Path(source)
    destination_path = Path(destination)
    if source_path.stat().st_size > MAX_CAPTURE_BYTES:
        raise ValueError("capture exceeds VAD input limit")
    with wave.open(str(source_path), "rb") as audio:
        if (
            audio.getnchannels() != 1
            or audio.getsampwidth() != 2
            or audio.getframerate() != 16000
            or audio.getcomptype() != "NONE"
        ):
            raise ValueError("unsupported PCM format")
        frames = audio.getnframes()
        samples = np.frombuffer(audio.readframes(frames), dtype="<i2").astype(np.float32)
        samples /= 32768.0

    VadOptions, collect_chunks, get_speech_timestamps = _load_vad()
    chunks = get_speech_timestamps(samples, VadOptions())
    if not chunks:
        return {"speech": False, "input_samples": int(samples.size), "output_samples": 0}
    collected = collect_chunks(samples, chunks)
    # Installed faster-whisper 1.2.1 returns (audio_tuple, segments) where
    # audio_tuple is a tuple of per-segment float32 arrays.
    audio_parts = collected[0] if isinstance(collected, tuple) else (collected,)
    speech = np.concatenate([np.asarray(part, dtype=np.float32).reshape(-1) for part in audio_parts])
    pcm = (np.clip(speech, -1.0, 1.0) * 32767.0).astype("<i2").tobytes()

    destination_path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=".vad-", suffix=".wav", dir=destination_path.parent)
    try:
        os.close(fd)
        with wave.open(temporary, "wb") as output:
            output.setnchannels(1)
            output.setsampwidth(2)
            output.setframerate(16000)
            output.writeframes(pcm)
        os.replace(temporary, destination_path)
    finally:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
    return {
        "speech": True,
        "input_samples": int(samples.size),
        "output_samples": len(pcm) // 2,
    }


def main() -> None:
    # Import/model preparation occurs once before readiness, so every subsequent
    # request reuses the installed Silero session instead of paying import/load
    # cost or creating a second recognizer.
    _load_vad()
    emit({"ready": True, "protocol": 1})
    for line in sys.stdin:
        request = None
        try:
            request = json.loads(line)
            if request.get("op") == "shutdown":
                emit({"shutdown": True})
                return
            if request.get("op") != "filter":
                raise ValueError("unsupported operation")
            result = filter_wav(str(request["input_wav"]), str(request["output_wav"]))
            result["id"] = request.get("id")
            emit(result)
        except Exception:
            # Fail closed without reflecting paths, audio, or exception internals.
            emit({"id": request.get("id") if isinstance(request, dict) else None, "error": "VAD failed"})


if __name__ == "__main__":
    main()
