"""Explicit hosted synthetic acceptance with nonzero exact Rust receipts."""

import os
import re
import subprocess
import sys


def require_hosted_fixture(environment):
    if (
        environment.get("GITHUB_ACTIONS") != "true"
        or environment.get("VESPER_NATIVE_CREDENTIAL_ACCEPTANCE")
        != "isolated-hosted-fixture"
        or environment.get("RUNNER_OS") not in {"Windows", "macOS", "Linux"}
    ):
        raise RuntimeError("native credential acceptance requires an isolated hosted fixture")


def has_exact_receipt(output, case):
    output = output.replace("\r\n", "\n")
    named = re.search(r"^test " + re.escape(case) + r" \.\.\. ok$", output, re.M)
    summary = re.search(
        r"^test result: ok\. 1 passed; 0 failed; 0 ignored;[^\n]*$", output, re.M
    )
    return named is not None and summary is not None


def main():
    require_hosted_fixture(os.environ)
    sys.stdout.reconfigure(encoding="utf-8")
    cases = [
        (
            ["-p", "vesper-auth", "--test", "native_persistence"],
            "hosted_native_credential_persistence",
        ),
        (
            ["-p", "vesper-provider-openai", "--lib"],
            "auth::tests::hosted_device_flow_persists_large_subscription_record",
        ),
    ]
    for selection, case in cases:
        command = [
            "cargo", "test", "--locked", *selection, case,
            "--", "--ignored", "--exact", "--test-threads=1", "--show-output",
        ]
        result = subprocess.run(
            command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            encoding="utf-8", errors="strict", timeout=240, check=False,
        )
        print(result.stdout, end="", flush=True)
        if result.returncode != 0 or not has_exact_receipt(result.stdout, case):
            raise RuntimeError(f"native credential acceptance lacks one passing exact case: {case}")
        print(f"native credential acceptance verified: {case}", flush=True)


if __name__ == "__main__":
    main()
