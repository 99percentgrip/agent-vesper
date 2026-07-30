# Agent Vesper Compatibility Fixtures

This tree is the language-neutral behavioral contract between the frozen
Python oracle at commit
`bf4d4287e2e3320aa3f09015f678e6169d520045` and later Rust consumers.

Each scenario directory contains a `manifest.json` and, after capture,
`result.python.json`. Manifests validate against
`schema/scenario-manifest-v1.schema.json`; results validate against
`schema/result-v1.schema.json`.

Categories:

- `acp/` — protocol lifecycle, replay, commands, cancellation, usage order.
- `provider/glm/` — request and deterministic local SSE/status behavior.
- `sessions/v1/` — sanitized schema-1/legacy/corrupt session compatibility.
- `tools/` — schemas, filesystem, patch/search/process outcomes.
- `policy/` — mode/policy/approval matrices.
- `security/` — redaction, promptware, signature, checkpoint invariants.
- `process/` — descendant, pipe, timeout, cancellation observations.

Comparison classes are `exact-output`, `semantic-parity`,
`schema-compatibility`, `security-invariant`, and `performance`.

Canonical JSON uses UTF-8, sorted object keys, compact separators, and one
terminal newline. `manifest-sha256.json` hashes every tracked fixture payload
except itself.

Run:

```text
python3 tools/python-oracle/oracle.py capture-all --source "/path/to/source"
python3 tools/python-oracle/oracle.py validate-all
python3 tools/python-oracle/oracle.py verify-index
```

The runner always redirects state and must reject a source commit mismatch.

