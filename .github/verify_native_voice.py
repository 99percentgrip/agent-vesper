"""Real local speech setup/signal on disposable native runners; no audio devices/providers."""
import hashlib
import os
from pathlib import Path
import platform
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


def run(args, env=None, timeout=900):
    result = subprocess.run(args, env=env, timeout=timeout, text=True,
                            encoding="utf-8", errors="replace", stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT)
    print(result.stdout, flush=True)
    if result.returncode:
        raise RuntimeError(f"native voice command failed ({result.returncode}): {args[0]}")
    return result.stdout


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
