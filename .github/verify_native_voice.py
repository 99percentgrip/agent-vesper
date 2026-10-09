"""Real local speech setup/signal on disposable native runners; no audio devices/providers."""
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tempfile
import time
import urllib.request


def download_verified(url, destination, size, expected_digest):
    started = time.monotonic()
    count = 0
    digest = hashlib.sha256()
    with urllib.request.urlopen(url, timeout=30) as source, destination.open("wb") as target:
        if not source.geturl().startswith("https://"):
            raise RuntimeError("native voice prerequisite redirected outside HTTPS")
        while chunk := source.read(64 * 1024):
            count += len(chunk)
            if count > size or time.monotonic() - started > 300:
                raise RuntimeError("native voice prerequisite exceeded download bounds")
            digest.update(chunk)
            target.write(chunk)
    if count != size or digest.hexdigest() != expected_digest:
        raise RuntimeError("native pronunciation package integrity failed")


def run(args, env=None, timeout=900, input_data=None):
    result = subprocess.run(args, env=env, timeout=timeout, input=input_data, text=True,
                            encoding="utf-8", errors="replace", stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT)
    print(result.stdout, flush=True)
    if result.returncode:
        raise RuntimeError(f"native voice command failed ({result.returncode}): {args[0]}")
    return result.stdout


def verify_recognition_receipt(output):
    # Imports, readiness and synthesis alone cannot establish recognition.
    values = []
    for line in output.splitlines():
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict):
            values.append(value)
    expected = {"speech", "recognition", "test", "hello", "world"}
    if len(values) != 5 or values[0] != {"ready": True}:
        raise RuntimeError("missing ordered production recognition receipt")
    for chunk, done in [(values[1], values[2]), (values[3], values[4])]:
        if (chunk.get("index") != 0 or not isinstance(chunk.get("text"), str)
                or len(expected.intersection(re.findall(r"[a-z]+", chunk["text"].lower()))) < 3
                or done != {"done": True, "chunks": 1}):
            raise RuntimeError("production recognition failed its fixed-phrase/reuse receipt")


def real_recognition(root, tools, env):
    model = root / "recognition model"
    model.mkdir()
    manifest = json.loads(Path(__file__).with_name("native-voice-stt-model.json").read_text(encoding="utf-8"))
    for entry in manifest["files"]:
        url = f'https://huggingface.co/{manifest["repository"]}/resolve/{manifest["revision"]}/{entry["name"]}'
        download_verified(url, model / entry["name"], entry["size"], entry["sha256"])
        print(f'RECOGNITION ASSET VERIFIED {entry["name"]} {entry["sha256"]}', flush=True)
    source = root / "fixed speech.wav"
    canonical = root / "canonical speech.wav"
    python = tools["python"]
    run([tools["espeak"], "-v", "en-us", "-s", "145", "-w", str(source),
         "This is a speech recognition test. Hello world."], env=env, timeout=30)
    # The production protocol expects mono s16 16 kHz. Fixed, nonsensitive
    # synthetic speech only; native capture/rate conversion have separate tests.
    run([python, "-c", """
import sys,wave,numpy as np
with wave.open(sys.argv[1],'rb') as src:
    assert src.getsampwidth()==2 and src.getnchannels()==1
    assert src.getnframes() <= 20*src.getframerate()
    rate=src.getframerate()
    samples=np.frombuffer(src.readframes(src.getnframes()),dtype='<i2')
assert samples.size and np.max(np.abs(samples.astype(np.int32)))>=256
out=np.interp(np.arange(round(samples.size*16000/rate))*rate/16000,np.arange(samples.size),samples)
with wave.open(sys.argv[2],'wb') as dst:
    dst.setnchannels(1);dst.setsampwidth(2);dst.setframerate(16000)
    dst.writeframes(np.rint(out).astype('<i2').tobytes())
""", str(source), str(canonical)], env=env, timeout=30)
    speech_env = env.copy()
    speech_env.update(GLM_ACP_WHISPER_MODEL=str(model), HF_HUB_OFFLINE="1",
                      HF_HOME=str(root / "huggingface"), PYTHONNOUSERSITE="1",
                      PYTHONDONTWRITEBYTECODE="1", PYTHONIOENCODING="utf-8")
    # Preserve the first library/VAD causal traceback on disposable runners;
    # the production sidecar intentionally emits only a safe generic failure.
    run([python, "-c", """
import sys,wave,numpy as np
from faster_whisper import WhisperModel
model=WhisperModel(sys.argv[1],device='cpu',compute_type='int8',cpu_threads=2)
with wave.open(sys.argv[2],'rb') as src:
    audio=np.frombuffer(src.readframes(src.getnframes()),dtype='<i2').astype(np.float32)/32768
segments,_=model.transcribe(audio,vad_filter=True)
text=' '.join(s.text.strip() for s in segments)
assert text.strip(), 'real library/VAD produced no speech'
print('REAL RECOGNIZER LIBRARY/VAD VERIFIED')
""", str(model), str(canonical)], env=speech_env, timeout=180)
    script = Path(__file__).resolve().parent.parent / "apps/agent-vesper-tui/src/voice_transcribe.py"
    request = json.dumps({"wav": str(canonical), "skip": 0}) + "\n"
    output = run([python, "-u", str(script)], env=speech_env, timeout=180,
                 input_data=request * 2)
    verify_recognition_receipt(output)
    print("NATIVE PRODUCTION RECOGNITION AND REUSE VERIFIED (no microphone/provider)")


def main():
    if os.environ.get("GITHUB_ACTIONS") != "true" or not os.environ.get("RUNNER_TEMP"):
        raise RuntimeError("voice setup acceptance requires a disposable GitHub runner")
    system = platform.system()
    with tempfile.TemporaryDirectory(prefix="vesper voice ", dir=os.environ["RUNNER_TEMP"]) as tmp:
        root = Path(tmp).resolve()
        if system == "Windows":
            installer = root / "espeak-ng.msi"
            download_verified(
                "https://github.com/espeak-ng/espeak-ng/releases/download/1.52.0/espeak-ng.msi", installer,
                12765862, "7f673c709ea5dd579d3b5ebb98688cc575328a6ab7438d2bc405b88cedaeafb9")
            run(["msiexec.exe", "/i", str(installer), "/qn", "/norestart"], timeout=300)
        elif system == "Darwin":
            run(["brew", "install", "espeak-ng"], timeout=600)
        elif system == "Linux":
            run(["sudo", "--preserve-env=GITHUB_ACTIONS,RUNNER_OS", sys.executable,
                 str(Path(__file__).with_name("prepare-linux-apt.py"))], timeout=30)
            run(["sudo", "apt-get", "update", "-qq", "-o", "Acquire::Retries=2",
                 "-o", "Acquire::http::Timeout=10", "-o", "Acquire::https::Timeout=10",
                 "-o", "Acquire::ForceIPv4=true"], timeout=120)
            run(["sudo", "apt-get", "install", "-y", "espeak-ng", "alsa-utils",
                 "-o", "Acquire::Retries=2", "-o", "Acquire::https::Timeout=10",
                 "-o", "Acquire::ForceIPv4=true"], timeout=180)
        else:
            raise RuntimeError("unsupported native release runner")
        env = os.environ.copy()
        # Keep the already prepared compiler while isolating application state.
        home = Path.home()
        env.setdefault("CARGO_HOME", str(home / ".cargo"))
        env.setdefault("RUSTUP_HOME", str(home / ".rustup"))
        for key in ("HOME", "USERPROFILE", "LOCALAPPDATA", "XDG_DATA_HOME", "XDG_CACHE_HOME", "UV_CACHE_DIR", "UV_PYTHON_INSTALL_DIR", "AGENT_VESPER_HOME"):
            destination = root / key
            destination.mkdir()
            env[key] = str(destination)
        output = run(["cargo", "run", "--locked", "-p", "vesper-harness", "--example",
                      "voice_setup_receipt", "--", str(root)], env=env)
        if output.count("NATIVE VOICE DEPENDENCIES VERIFIED") != 1:
            raise RuntimeError("missing exact production dependency receipt")
        tools = [json.loads(line.removeprefix("NATIVE VOICE TOOLS "))
                 for line in output.splitlines() if line.startswith("NATIVE VOICE TOOLS ")]
        if len(tools) != 1:
            raise RuntimeError("missing verified native speech executable receipt")
        real_recognition(root, tools[0], env)
        output = run(["cargo", "run", "--locked", "-p", "vesper-voice-kokoro", "--features",
                      "ort", "--example", "real_setup_receipt"], env=env)
        if "POST-INSTALL ASSESSMENT: Ready" not in output:
            raise RuntimeError("production pack did not verify")
        output = run(["cargo", "run", "--locked", "-p", "vesper-voice-kokoro", "--features",
                      "ort", "--example", "signal_receipt"], env=env)
        if output.count("voice=am_michael ") != 1 or output.count("voice=af_heart ") != 1:
            raise RuntimeError("missing real signal receipt for a shipped voice")
        print("NATIVE VOICE SETUP AND BOTH SIGNALS VERIFIED (no microphone/speaker proof)")


if __name__ == "__main__":
    main()
