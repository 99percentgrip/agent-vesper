#!/usr/bin/env python3
"""Language-neutral fixture capture coordinator for the frozen Python source."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import signal
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

SOURCE_COMMIT = "bf4d4287e2e3320aa3f09015f678e6169d520045"
RUNNER_VERSION = "1.0.0"
CANARY = "VESPER_SECRET_CANARY_7xQ9m2Kp"
ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "fixtures"
MANIFEST_SCHEMA = FIXTURES / "schema" / "scenario-manifest-v1.schema.json"
RESULT_SCHEMA = FIXTURES / "schema" / "result-v1.schema.json"
INDEX_PATH = FIXTURES / "manifest-sha256.json"

SCENARIOS: tuple[tuple[str, str, str], ...] = (
    # ACP
    ("acp.initialization", "acp", "exact-output"),
    ("acp.capability-negotiation", "acp", "exact-output"),
    ("acp.new-session", "acp", "exact-output"),
    ("acp.load-session", "acp", "exact-output"),
    ("acp.resume-session", "acp", "exact-output"),
    ("acp.fork-session", "acp", "exact-output"),
    ("acp.list-session", "acp", "exact-output"),
    ("acp.close-session", "acp", "exact-output"),
    ("acp.replay-order", "acp", "exact-output"),
    ("acp.slash-command", "acp", "exact-output"),
    ("acp.cancellation", "acp", "exact-output"),
    ("acp.usage-update-order", "acp", "exact-output"),
    # GLM transport
    ("glm.request-serialization", "provider/glm", "exact-output"),
    ("glm.reasoning-then-content", "provider/glm", "exact-output"),
    ("glm.content-only", "provider/glm", "exact-output"),
    ("glm.fragmented-tool-call", "provider/glm", "exact-output"),
    ("glm.interleaved-tool-indexes", "provider/glm", "exact-output"),
    ("glm.usage-only-chunk", "provider/glm", "exact-output"),
    ("glm.malformed-sse-line", "provider/glm", "exact-output"),
    ("glm.blank-comment-lines", "provider/glm", "exact-output"),
    ("glm.done-marker", "provider/glm", "exact-output"),
    ("glm.terminal-finish-reason", "provider/glm", "exact-output"),
    ("glm.incomplete-eof-no-output", "provider/glm", "exact-output"),
    ("glm.incomplete-eof-visible-output", "provider/glm", "exact-output"),
    ("glm.retryable-status", "provider/glm", "semantic-parity"),
    ("glm.non-retryable-status", "provider/glm", "exact-output"),
    ("glm.retry-after-numeric", "provider/glm", "semantic-parity"),
    ("glm.retry-after-date", "provider/glm", "semantic-parity"),
    ("glm.cancel-before-connect", "provider/glm", "exact-output"),
    ("glm.cancel-before-headers", "provider/glm", "semantic-parity"),
    ("glm.cancel-mid-stream", "provider/glm", "semantic-parity"),
    ("glm.output-length-continuation", "provider/glm", "exact-output"),
    ("glm.continuation-cap", "provider/glm", "exact-output"),
    # Sessions
    ("session.schema1-complete", "sessions/v1", "schema-compatibility"),
    ("session.minimal-legacy", "sessions/v1", "schema-compatibility"),
    ("session.unknown-fields", "sessions/v1", "schema-compatibility"),
    ("session.corrupt-json", "sessions/v1", "schema-compatibility"),
    ("session.replay-and-lineage", "sessions/v1", "schema-compatibility"),
    ("session.reasoning-enabled", "sessions/v1", "security-invariant"),
    ("session.reasoning-disabled", "sessions/v1", "security-invariant"),
    # Tools
    ("tool.canonical-schemas", "tools", "exact-output"),
    ("tool.path-containment", "tools", "security-invariant"),
    ("tool.symlink-escape", "tools", "security-invariant"),
    ("tool.utf8-and-binary", "tools", "semantic-parity"),
    ("tool.read-write-edit-patch", "tools", "semantic-parity"),
    ("tool.patch-set-atomic", "tools", "security-invariant"),
    ("tool.search-bounds", "tools", "semantic-parity"),
    ("tool.command-timeout", "tools", "security-invariant"),
    ("tool.command-cancellation", "tools", "security-invariant"),
    ("tool.descendant-cleanup", "tools", "security-invariant"),
    # Policy
    ("policy.matrix", "policy", "security-invariant"),
    ("policy.bypass-deny", "policy", "security-invariant"),
    ("policy.readonly-destructive", "policy", "security-invariant"),
    ("policy.ask-channel-failure", "policy", "security-invariant"),
    ("policy.plan-mcp", "policy", "exact-output"),
    ("policy.nested-workflow-denial", "policy", "security-invariant"),
    # Security
    ("security.secret-redaction", "security", "security-invariant"),
    ("security.promptware-wrapping", "security", "security-invariant"),
    ("security.plugin-signature", "security", "security-invariant"),
    ("security.checkpoint-conflict", "security", "security-invariant"),
    ("security.canary-sinks", "security", "security-invariant"),
    # Process corpus (Python source baseline; Rust spike has richer native probes)
    ("process.direct-child", "process", "security-invariant"),
    ("process.grandchild", "process", "security-invariant"),
    ("process.pipe-holder", "process", "security-invariant"),
    ("process.huge-output", "process", "performance"),
)


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n").encode()


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(canonical_bytes(value))


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def validate(instance: Any, schema_path: Path) -> None:
    try:
        import jsonschema
    except ImportError as error:
        raise SystemExit("jsonschema is required by the coordinator environment") from error
    jsonschema.Draft202012Validator(load_json(schema_path)).validate(instance)


def source_head(source: Path) -> str:
    completed = subprocess.run(
        ["git", "-C", str(source), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
        timeout=10,
    )
    return completed.stdout.strip()


def scenario_dir(scenario_id: str, category: str) -> Path:
    leaf = scenario_id.split(".", 1)[1]
    return FIXTURES / category / leaf


def normalize(value: Any, workspace: str, temp_root: str) -> Any:
    uuid_map: dict[str, str] = {}
    pid_map: dict[str, str] = {}
    uuid_re = re.compile(r"\b[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\b", re.I)
    pid_re = re.compile(r"(?<=pid[=: ])\d+", re.I)

    def text(value: str) -> str:
        value = value.replace(workspace, "$WORKSPACE").replace(temp_root, "$TMP")
        value = uuid_re.sub(lambda m: uuid_map.setdefault(m.group(0), f"$UUID{len(uuid_map) + 1}"), value)
        value = pid_re.sub(lambda m: pid_map.setdefault(m.group(0), f"$PID{len(pid_map) + 1}"), value)
        return value

    def walk(item: Any) -> Any:
        if isinstance(item, str):
            return text(item)
        if isinstance(item, list):
            return [walk(entry) for entry in item]
        if isinstance(item, dict):
            return {str(key): walk(entry) for key, entry in item.items()}
        return item

    return walk(value)


def build_manifest(
    scenario_id: str, category: str, comparison: str, result: dict[str, Any]
) -> dict[str, Any]:
    return {
        "scenario_id": scenario_id,
        "schema_version": 1,
        "category": category,
        "comparison_class": comparison,
        "source_commit": SOURCE_COMMIT,
        "platform_requirements": {"os": ["any"], "arch": ["any"], "capabilities": []},
        "input": {"probe": scenario_id},
        "environment": {"network": "loopback-only", "seed": 424242},
        "fixture_files": [],
        "expected_events": result["events"],
        "expected_state": result["final_state"],
        "expected_persistence": {
            "persisted_files": result["persisted_files"],
            "file_hashes": result["file_hashes"],
            "file_modes_or_acl_status": result["file_modes_or_acl_status"],
        },
        "expected_process_observations": result["process_observations"],
        "expected_network_observations": result["network_observations"],
        "normalization_rules": [
            {"target": "**", "rule": "uuid-encounter-order"},
            {"target": "**", "rule": "workspace-root"},
            {"target": "**", "rule": "temporary-root"},
            {"target": "**", "rule": "pid-encounter-order"},
        ],
        "security_assertions": [
            {"kind": "secret-canary-absent", "expected": True, "details": {"canary": "$CANARY"}}
        ],
        "timeout": {"seconds": 30, "category": "runner"},
    }


def worker_command(source: Path, scenario_id: str, workspace: Path) -> list[str]:
    python = source / ".venv" / "bin" / "python3"
    if not python.exists():
        raise SystemExit(f"source virtualenv Python not found: {python}")
    return [
        str(python),
        str(Path(__file__).with_name("source_worker.py")),
        "--source",
        str(source),
        "--scenario",
        scenario_id,
        "--workspace",
        str(workspace),
    ]


def capture_one(source: Path, item: tuple[str, str, str]) -> None:
    scenario_id, category, comparison = item
    run_root = Path("/tmp/agent-vesper-python-oracle") / scenario_id.replace(".", "-")
    if run_root.exists():
        shutil.rmtree(run_root)
    run_root.mkdir(parents=True)
    workspace = run_root / "workspace"
    workspace.mkdir()
    for name in ("home", "config", "cache", "pycache", "tmp"):
        (run_root / name).mkdir()
    env = os.environ.copy()
    env.update(
        {
            "HOME": str(run_root / "home"),
            "XDG_CONFIG_HOME": str(run_root / "config"),
            "XDG_CACHE_HOME": str(run_root / "cache"),
            "PYTHONPYCACHEPREFIX": str(run_root / "pycache"),
            "TMPDIR": str(run_root / "tmp"),
            "PYTHONDONTWRITEBYTECODE": "1",
            "GLM_ACP_CRON_DISABLE": "1",
            "GLM_ACP_SESSION_PERSISTENCE": "1",
            "ZAI_API_KEY": CANARY,
            "PYTHONHASHSEED": "424242",
        }
    )
    process = subprocess.Popen(
        worker_command(source, scenario_id, workspace),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
        start_new_session=True,
    )
    try:
        stdout, stderr = process.communicate(timeout=30)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        stdout, stderr = process.communicate()
        raise RuntimeError(f"{scenario_id}: worker timeout")
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
    if CANARY in stdout or CANARY in stderr:
        raise RuntimeError(f"{scenario_id}: secret canary leaked")
    if process.returncode:
        raise RuntimeError(f"{scenario_id}: worker failed: {stderr[-2000:]}")
    raw = json.loads(stdout)
    raw = normalize(raw, str(workspace), str(run_root))
    result = {
        "scenario_id": scenario_id,
        "runner": "python-oracle",
        "runner_version": RUNNER_VERSION,
        "platform": {
            "os": platform.system().lower(),
            "arch": platform.machine().lower(),
            "runtime": raw.pop("runtime"),
        },
        "events": raw.pop("events"),
        "final_state": raw.pop("final_state"),
        "persisted_files": raw.pop("persisted_files"),
        "file_hashes": raw.pop("file_hashes"),
        "file_modes_or_acl_status": raw.pop("file_modes_or_acl_status"),
        "process_observations": raw.pop("process_observations"),
        "network_observations": raw.pop("network_observations"),
        "logs": raw.pop("logs"),
        "redaction_assertions": [
            {
                "canary": "$CANARY",
                "absent": True,
                "locations_checked": ["stdout", "stderr", "canonical-result", "persisted-files"],
            }
        ],
        "duration_metadata": {"class": "bounded", "milliseconds": 0},
        "result": {
            "status": "pass",
            "classification": "reproduced",
            "message": raw.pop("message", "captured from frozen source"),
        },
    }
    if raw:
        raise RuntimeError(f"{scenario_id}: unconsumed worker fields: {sorted(raw)}")
    encoded = canonical_bytes(result)
    if CANARY.encode() in encoded:
        raise RuntimeError(f"{scenario_id}: canary leaked after normalization")
    validate(result, RESULT_SCHEMA)
    manifest = build_manifest(scenario_id, category, comparison, result)
    validate(manifest, MANIFEST_SCHEMA)
    output_dir = scenario_dir(scenario_id, category)
    write_json(output_dir / "manifest.json", manifest)
    write_json(output_dir / "result.python.json", result)
    shutil.rmtree(run_root)


def fixture_payloads() -> list[Path]:
    return sorted(
        path
        for path in FIXTURES.rglob("*")
        if path.is_file()
        and path != INDEX_PATH
        and path.name != "AGENTS.md"
        and not path.name.startswith("coverage-stage")
        and path.suffix in {".json", ".jsonl"}
    )


def build_index() -> dict[str, Any]:
    return {
        "schema_version": 1,
        "source_commit": SOURCE_COMMIT,
        "files": {
            path.relative_to(ROOT).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in fixture_payloads()
        },
    }


def validate_all() -> None:
    manifests = sorted(path for path in FIXTURES.rglob("manifest.json") if path.is_file())
    for manifest_path in manifests:
        directory = manifest_path.parent
        manifest = load_json(manifest_path)
        result = load_json(directory / "result.python.json")
        validate(manifest, MANIFEST_SCHEMA)
        validate(result, RESULT_SCHEMA)
        if manifest["scenario_id"] != result["scenario_id"]:
            raise SystemExit(f"scenario mismatch at {directory}")
    print(f"validated {len(manifests)} scenarios", file=sys.stderr)


def verify_index() -> None:
    actual = load_json(INDEX_PATH)
    expected = build_index()
    if actual != expected:
        raise SystemExit("fixture hash index mismatch")
    print(f"verified {len(expected['files'])} fixture payload hashes", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    capture = sub.add_parser("capture-all")
    capture.add_argument("--source", type=Path, required=True)
    capture.add_argument("--start", type=int, default=1)
    sub.add_parser("validate-all")
    sub.add_parser("verify-index")
    sub.add_parser("rebuild-index")
    args = parser.parse_args()
    if args.command == "capture-all":
        source = args.source.resolve()
        if source_head(source) != SOURCE_COMMIT:
            raise SystemExit("source commit mismatch")
        for index, item in enumerate(SCENARIOS, 1):
            if index < args.start:
                continue
            capture_one(source, item)
            print(f"[{index}/{len(SCENARIOS)}] {item[0]}", file=sys.stderr)
        validate_all()
        write_json(INDEX_PATH, build_index())
        verify_index()
    elif args.command == "validate-all":
        validate_all()
    elif args.command == "verify-index":
        verify_index()
    else:
        validate_all()
        write_json(INDEX_PATH, build_index())
        verify_index()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
