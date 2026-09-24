#!/usr/bin/env python3
"""VRO-17 PR-2 no-speaker probe (evidence helper, not production code).

Drives the REAL installed system speech engine exactly as the Rust
adapter does (fixed argv, text on stdin, --stdout capture) and measures:
actual audio format, duration, first-output vs total-generation timing,
and storage behavior. No speaker activation: --stdout never touches an
audio device. Probe artifacts are bounded (<=8 MiB/file, <=32 MiB
aggregate) in a private directory and cleaned afterward; receipts are
JSON only. Nonsensitive fixed test text.
"""
import json
import os
import shutil
import subprocess
import time

ENGINE = os.environ.get("VESPER_TTS_ENGINE", "espeak-ng")
PROBE_DIR = "/tmp/vesper-voice-pr2-probe"
OUT = "docs/foundation/voice-oracle-pr2-probe-results.json"
LIMITS = {"per_file": 8 * 1024 * 1024, "aggregate": 32 * 1024 * 1024}
TEXTS = {
    "short": "Testing synthesis.",
    "sentence-pair": "First sentence here. Second sentence follows.",
    "code-skipped": "Before code. ```print('hidden')``` After code.",
}


def bytes_avail(path):
    st = os.statvfs(path)
    return st.f_bavail * st.f_frsize


def dir_size(path):
    total = 0
    for root, _dirs, files in os.walk(path):
        for name in files:
            try:
                total += os.path.getsize(os.path.join(root, name))
            except OSError:
                pass
    return total


def parse_wav(raw):
    if len(raw) < 44 or raw[0:4] != b"RIFF" or raw[8:12] != b"WAVE":
        return None
    import struct

    audiofmt, channels, rate, _br, _ba, bits = struct.unpack(
        "<HHIIHH", raw[20:36]
    )
    return {
        "audio_format": audiofmt,
        "channels": channels,
        "sample_rate_hz": rate,
        "bits": bits,
        "riff_size_field": struct.unpack("<I", raw[4:8])[0],
        "data_size_field": struct.unpack("<I", raw[40:44])[0],
        "actual_bytes": len(raw),
        "duration_s": round((len(raw) - 44) / 2 / channels / rate, 3),
    }


def main():
    before_avail_tmp = bytes_avail("/tmp")
    if os.path.exists(PROBE_DIR):
        shutil.rmtree(PROBE_DIR)  # owned dir only; bounded from prior runs
    os.makedirs(PROBE_DIR)
    runs = []
    for name, text in TEXTS.items():
        proc = subprocess.Popen(
            [ENGINE, "-v", "en", "--stdin", "--stdout"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
        t0 = time.perf_counter()
        # First-output timing: read the first stdout chunk as it arrives.
        assert proc.stdin is not None and proc.stdout is not None
        proc.stdin.write(text.encode())
        proc.stdin.close()
        first = proc.stdout.read1(65536)
        t_first = time.perf_counter() - t0
        rest = proc.stdout.read()
        t_total = time.perf_counter() - t0
        proc.wait(timeout=10)
        raw = first + rest
        probe_path = os.path.join(PROBE_DIR, f"{name}.wav")
        assert len(raw) <= LIMITS["per_file"], f"probe file exceeds bound: {len(raw)}"
        with open(probe_path, "wb") as handle:
            handle.write(raw)
        facts = parse_wav(raw)
        runs.append(
            {
                "text_kind": name,
                "engine": ENGINE,
                "engine_version": subprocess.run(
                    [ENGINE, "--version"], capture_output=True, text=True
                ).stdout.strip(),
                "first_output_s": round(t_first, 4),
                "total_generation_s": round(t_total, 4),
                "exit_code": proc.returncode,
                "wav": facts,
                "note": "audio proves synthesis output only; no listening/intelligibility check performed",
            }
        )
        print(json.dumps(runs[-1], ensure_ascii=False), flush=True)
    aggregate = dir_size(PROBE_DIR)
    assert aggregate <= LIMITS["aggregate"], aggregate
    # Cleanup owned probe artifacts; keep receipts only.
    shutil.rmtree(PROBE_DIR)
    after_avail_tmp = bytes_avail("/tmp")
    receipt = {
        "runs": runs,
        "storage": {
            "probe_dir": PROBE_DIR,
            "peak_aggregate_bytes": aggregate,
            "residual_after_cleanup": dir_size(PROBE_DIR) if os.path.exists(PROBE_DIR) else 0,
            "tmp_avail_before_bytes": before_avail_tmp,
            "tmp_avail_after_bytes": after_avail_tmp,
            "per_file_limit": LIMITS["per_file"],
            "aggregate_limit": LIMITS["aggregate"],
        },
    }
    with open(OUT, "w") as handle:
        json.dump(receipt, handle, indent=2, ensure_ascii=False)
    print("PROBE COMPLETE; artifacts cleaned; receipts:", OUT)


if __name__ == "__main__":
    main()
