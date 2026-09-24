#!/usr/bin/env python3
"""VRO-17 PR-1 real-model probe (evidence helper, not production code).

Drives the REAL shipped script exactly as the Rust adapter does
(`python -c SCRIPT`, `GLM_ACP_WHISPER_MODEL` env) against the REAL
faster-whisper models already cached on this machine. Fixtures are
synthetic and deterministic; no recordings of Alex are used.

Fixtures:
  - digital silence 30 s (zeros)
  - long digital silence 90 s (the trailing-silence hallucination shape)
  - tone burst 30 s (1 s of 440 Hz inside silence; NOT intelligible
    speech — recorded honestly as a non-speech acoustic input, never
    claimed as a recognition-accuracy receipt)

Output: JSON receipts to docs/foundation/voice-oracle-pr1-probe-results.json.
"""
import json
import math
import os
import subprocess
import time
import wave

VENV_PY = os.environ.get(
    "VESPER_VOICE_VENV_PY",
    os.path.expanduser("~/.local/share/agent-vesper/voice-venv/bin/python"),
)
SCRIPT = "apps/agent-vesper-tui/src/voice_transcribe.py"
FIXTURES = "/tmp/vesper-voice-pr1-probe"
OUT = "docs/foundation/voice-oracle-pr1-probe-results.json"


def write_wav(path, seconds, gen):
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(16000)
        w.writeframes(b"".join(gen(i) for i in range(seconds * 16000)))


def zero(_i):
    return (0).to_bytes(2, "little")


def tone_burst(i):
    second = i // 16000
    if 8 <= second < 9 and (i % 400) < 200:
        sample = int(12000 * math.sin(2 * math.pi * 440 * i / 16000))
        return sample.to_bytes(2, "little", signed=True)
    return (0).to_bytes(2, "little")


def main():
    os.makedirs(FIXTURES, exist_ok=True)
    cases = [
        ("silence-30s", f"{FIXTURES}/silence30.wav", 30, zero),
        ("long-silence-90s", f"{FIXTURES}/silence90.wav", 90, zero),
        ("tone-burst-30s", f"{FIXTURES}/tone30.wav", 30, tone_burst),
    ]
    for name, path, seconds, gen in cases:
        write_wav(path, seconds, gen)
        print(f"wrote {name}: {os.path.getsize(path)} bytes", flush=True)

    script_source = open(SCRIPT).read()
    results = []
    for model in ["tiny", "base"]:
        for name, path, seconds, _ in cases:
            request = json.dumps({"wav": path, "skip": 0}) + "\n"
            proc = subprocess.Popen(
                [VENV_PY, "-c", script_source],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                env={**dict(os.environ), "GLM_ACP_WHISPER_MODEL": model},
            )
            t0 = time.perf_counter()
            out, _ = proc.communicate(request.encode(), timeout=900)
            elapsed = time.perf_counter() - t0
            texts = []
            for line in out.decode().splitlines():
                try:
                    value = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if "text" in value:
                    texts.append(value["text"])
                if value.get("done"):
                    break
            joined = " ".join(texts).strip()
            receipt = {
                "model": model,
                "vad": "vad_filter=True (shipped script default)",
                "fixture": name,
                "seconds": seconds,
                "wall_s": round(elapsed, 2),
                "empty": joined == "",
                "text_len": len(joined),
                "text_head": joined[:60],
                "source": "real model (cached faster-whisper) via real script protocol",
            }
            results.append(receipt)
            print(json.dumps(receipt, ensure_ascii=False), flush=True)
    with open(OUT, "w") as f:
        json.dump({"runs": results}, f, indent=2, ensure_ascii=False)
    print("PROBE COMPLETE")


if __name__ == "__main__":
    main()
