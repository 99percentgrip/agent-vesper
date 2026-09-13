"""Private local PCM chunk transcription; stdout is a bounded JSON protocol."""
import json
import os
import sys
import wave


def emit(value):
    print(json.dumps(value), flush=True)


def transcribe(path, skip, model):
    import numpy as np
    with wave.open(path, "rb") as audio:
        if audio.getsampwidth() != 2 or audio.getframerate() != 16000:
            raise ValueError("unsupported PCM format")
        channels = audio.getnchannels()
        chunk_frames = 30 * 16000
        index = 0
        while True:
            raw = audio.readframes(chunk_frames)
            if not raw:
                break
            if index >= skip:
                samples = np.frombuffer(raw, dtype="<i2").astype(np.float32) / 32768.0
                if channels > 1:
                    samples = samples.reshape(-1, channels).mean(axis=1)
                segments, _ = model.transcribe(samples)
                text = " ".join(segment.text.strip() for segment in segments).strip()
                emit({"index": index, "text": text, "seconds": min((index + 1) * 30, audio.getnframes() // 16000)})
            index += 1
        if skip > index:
            raise ValueError("invalid resume offset")
    emit({"done": True, "chunks": index})


def main():
    try:
        from faster_whisper import WhisperModel
        model = WhisperModel(os.environ.get("GLM_ACP_WHISPER_MODEL", "base"), device="cpu", compute_type="int8")
        emit({"ready": True})
        for line in sys.stdin:
            request = json.loads(line)
            transcribe(request["wav"], int(request["skip"]), model)
    except Exception:
        emit({"error": "transcription failed"})


if __name__ == "__main__":
    main()
