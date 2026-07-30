# Python Oracle and Critical Fixture Corpus

Status: COMPLETE

## Objective

Create an isolated, language-neutral capture runner and a reproducible
critical-path corpus from frozen source commit
`bf4d4287e2e3320aa3f09015f678e6169d520045`.

## Methods

`tools/python-oracle/oracle.py` runs under the coordinator Python (with
JSON Schema validation), validates the source Git commit, creates fixed
scenario-specific state under `/tmp/agent-vesper-python-oracle`, and launches
`source_worker.py` with the source `.venv` Python in a new process session.
The worker receives isolated HOME/config/cache/pycache/tmp roots, deterministic
hash seed, disabled cron, and a synthetic API-key canary.

The worker imports source modules directly for ACP/session/policy/security
state and tool schemas; it drives `GlmClient` through a loopback-only
`ThreadingHTTPServer`; and it runs narrow existing pytest selectors for
selected complex tool/plugin/checkpoint behaviors. It never contacts a remote
provider.

Worker stdout is one JSON object. The coordinator sanitizes paths/UUIDs/PIDs,
rejects the canary in stdout/stderr/result, validates both schemas, writes
canonical manifests/results, and rebuilds the SHA-256 index. A timeout kills
the worker process group.

## Fixture counts

| Category | Scenarios |
|---|---:|
| ACP | 12 |
| GLM provider/transport | 21 |
| Sessions v1 | 7 |
| Tools | 10 |
| Policy | 6 |
| Security | 5 |
| Process | 4 |
| **Total** | **65** |

There are **132 indexed JSON payloads**: two schemas plus 65 manifests and 65
Python results. The index itself is excluded from its own hash.

## Exact source evidence exercised

- ACP initialize/capabilities/session lifecycle/replay/cancel/usage:
  `agent.py:1839-2180,2540-2550,4056-4079,6790-6823`.
- GLM request/stream/retry/cancel/continuation:
  `glm_client.py:111-230,519-804`.
- Session schema and storage:
  `agent.py:555-673`, `session_store.py:56-245`.
- Tool schemas/containment/process:
  `tools.py:205-1117,1165-1218,1975-2082`.
- Policy ordering:
  `agent.py:4772-4933`, `policy.py:12-78`.
- Promptware/index redaction/signatures/checkpoints:
  `security.py:22-112`, `session_store.py:115-164`, focused
  `test_hardening_roadmap.py`, `test_safety_roadmap.py`, and
  `test_extensions.py` selectors.

## Commands and results

- `python3 -m py_compile tools/python-oracle/oracle.py tools/python-oracle/source_worker.py`
- Repeated bounded `python3 tools/python-oracle/oracle.py capture-all --source '/home/alex/Projects/Native GLM-5.2 Provider'` while fixing runner-only capture defects.
- `python3 tools/python-oracle/oracle.py validate-all`:
  **validated 65 scenarios**.
- `python3 tools/python-oracle/oracle.py verify-index`:
  **verified 132 fixture payload hashes**.
- Two final captures of the previously volatile process subset produced the
  identical complete index:
  `27e58c39fe95882961bf877b132b4ecbc6209850c57cd801fc2219e345632f86`.
  Earlier non-matching runs led to fixed scenario roots, fixed source
  timestamps, exclusion of derived SQLite/PID artifacts, and deterministic
  child-start synchronization.

## Reproduced source finding

Python command timeout kills/reaps the whole process group: the grandchild
fixture records zero survivors after `Command timed out after 1.0s`.

Python task cancellation is different: cancelling the `execute_tool` task
leaves both observed descendants alive, and the oracle must explicitly clean
them up. Evidence:
`fixtures/tools/command-cancellation/result.python.json` records
`surviving_descendants_before_oracle_cleanup: 2`. This follows from
`tools.py:2052-2067`, where process-group kill exists only in the
`asyncio.TimeoutError` branch and no `CancelledError` cleanup branch exists.
The fixture preserves the defect as source behavior; Rust security parity must
intentionally strengthen it to zero survivors.

## Files created

- `tools/python-oracle/{AGENTS.md,oracle.py,source_worker.py}`
- `fixtures/{AGENTS.md,README.md,manifest-sha256.json}`
- `fixtures/schema/*.schema.json`
- 65 scenario directories and 130 manifest/result files under the required
  category directories.
- This report.

## Security and isolation result

- Canary absent from captured stdout, stderr, canonical results, and persisted
  fixture files: **reproduced**.
- Source commit validation and nonzero timeout/schema/leak/canary handling:
  **locally validated**.
- No live provider endpoint used: **confirmed by loopback server request
  observations**.
- Process cancellation leak is recorded, then cleaned by the oracle so no
  child remains.

## Limitations and readiness

Some complex tool/security fixtures intentionally record a focused source test
outcome rather than a complete internal trace. They are suitable as foundation
canaries but later subsystem implementation must add richer cross-language
state fixtures before its parity gate. macOS/Windows normalization and ACL
fields remain CI validation pending.

The oracle and critical corpus no longer block the disposable Rust spikes.

