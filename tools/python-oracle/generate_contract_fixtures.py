#!/usr/bin/env python3
"""Generate deterministic provider-neutral Stage 2 contract vectors."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any

SOURCE_COMMIT = "bf4d4287e2e3320aa3f09015f678e6169d520045"
RUNNER_VERSION = "1.0.0"

VECTORS: dict[str, dict[str, Any]] = {
    "contract.acp-message-id-linkage": {
        "message_id": "message-1",
        "prompt_correlation_id": "correlation-1",
        "compatibility_note": "userMessageId placement remains adapter-owned",
    },
    "contract.command-event-correlation": {
        "command_id": "command-1",
        "correlation_id": "correlation-1",
        "event_id": "event-1",
        "session_id": "session-1",
        "turn_id": "turn-1",
    },
    "contract.error-redaction": {
        "category": "transport",
        "safe_message": "provider request failed",
        "visible_output_emitted": False,
        "forbidden_fields": ["authorization", "api_key", "request_body", "raw_url"],
    },
    "contract.fallback-observable": {
        "capability": "provider:vision",
        "requirement": "allow-fallback",
        "resolution": "fallback",
        "observable": True,
    },
    "contract.fragmented-parallel-tool-identity": {
        "fragments": [
            {"index": 0, "call_id": "call-a", "arguments": "{\"x\":"},
            {"index": 1, "call_id": "call-b", "arguments": "{\"y\":"},
            {"index": 0, "call_id": "call-a", "arguments": "1}"},
            {"index": 1, "call_id": "call-b", "arguments": "2}"},
        ],
        "assembled": {"call-a": {"x": 1}, "call-b": {"y": 2}},
    },
    "contract.invalid-session-bound": {
        "field": "task_context",
        "maximum": 2000,
        "actual": 2001,
        "outcome": "bounded-value-error",
    },
    "contract.opaque-reasoning-continuation": {
        "kind": "opaque-continuation",
        "displayable": False,
        "retention": "persist",
        "provider_namespace": "provider.synthetic",
        "round_trip_required": True,
    },
    "contract.terminal-uniqueness": {
        "stream": ["response-started", "content-delta", "turn-completed"],
        "second_terminal_outcome": "duplicate-terminal-error",
    },
    "contract.unknown-extension-roundtrip": {
        "extension": {"future.example:field": {"preserve": [1, 2, 3]}},
        "round_trip_required": True,
    },
    "contract.unknown-finish-reason": {
        "normalized": "unknown",
        "raw_provider_value": "future_finish_reason",
    },
    "contract.usage-provenance-modes": {
        "updates": [
            {"mode": "delta", "input": {"value": 3, "provenance": "exact"}},
            {"mode": "cumulative", "input": {"value": 7, "provenance": "estimated"}},
            {"mode": "delta", "input": {"value": None, "provenance": "unavailable"}},
        ]
    },
}


def canonical_bytes(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        + "\n"
    ).encode()


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(canonical_bytes(value))


def result(scenario_id: str, state: dict[str, Any]) -> dict[str, Any]:
    return {
        "scenario_id": scenario_id,
        "runner": "contract-generator",
        "runner_version": RUNNER_VERSION,
        "platform": {"os": "any", "arch": "any", "runtime": "language-neutral"},
        "events": [{"seq": 0, "type": "contract-vector", "data": state}],
        "final_state": state,
        "persisted_files": [],
        "file_hashes": {},
        "file_modes_or_acl_status": {},
        "process_observations": {"spawned": False},
        "network_observations": {"requests": 0},
        "logs": [],
        "redaction_assertions": [
            {
                "canary": "$CANARY",
                "absent": True,
                "locations_checked": ["canonical-result"],
            }
        ],
        "duration_metadata": {"class": "instant", "milliseconds": 0},
        "result": {
            "status": "pass",
            "classification": "locally-validated",
            "message": "synthetic future contract not expressible by frozen source",
        },
    }


def manifest(scenario_id: str, output: dict[str, Any]) -> dict[str, Any]:
    state = output["final_state"]
    return {
        "scenario_id": scenario_id,
        "schema_version": 1,
        "category": "contracts",
        "comparison_class": "exact-output",
        "source_commit": SOURCE_COMMIT,
        "platform_requirements": {"os": ["any"], "arch": ["any"], "capabilities": []},
        "input": {
            "synthetic_future_contract": True,
            "contract": scenario_id.split(".", 1)[1],
        },
        "environment": {"network": "disabled", "seed": 424242},
        "fixture_files": [],
        "expected_events": output["events"],
        "expected_state": state,
        "expected_persistence": {
            "persisted_files": [],
            "file_hashes": {},
            "file_modes_or_acl_status": {},
        },
        "expected_process_observations": {"spawned": False},
        "expected_network_observations": {"requests": 0},
        "normalization_rules": [],
        "security_assertions": [
            {
                "kind": "secret-canary-absent",
                "expected": True,
                "details": {"canary": "$CANARY"},
            }
        ],
        "timeout": {"seconds": 5, "category": "runner"},
    }


def generate(output_root: Path) -> str:
    hashes: dict[str, str] = {}
    for scenario_id, state in sorted(VECTORS.items()):
        directory = output_root / scenario_id.split(".", 1)[1]
        generated_result = result(scenario_id, state)
        values = {
            "manifest.json": manifest(scenario_id, generated_result),
            "result.python.json": generated_result,
        }
        for name, value in values.items():
            path = directory / name
            payload = canonical_bytes(value)
            write_json(path, value)
            hashes[path.relative_to(output_root).as_posix()] = hashlib.sha256(
                payload
            ).hexdigest()
    return hashlib.sha256(canonical_bytes(hashes)).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    print(generate(args.output.resolve()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
