# Language-Neutral Fixture Charter

Status: COMPLETE

## Objective

Define the stable scenario and result contracts used to compare the frozen
Python harness with disposable Rust spikes and later production Rust code.

## Methods and inspected evidence

The schema is derived from the event, provider, persistence, policy, and
security contracts in `docs/recon/`, particularly source behavior at
`agent.py:1839-2560,2836-3992,6759-6950`,
`glm_client.py:519-801`, `session_store.py:56-413`,
`tools.py:1125-2082`, and `agent.py:4772-5092`.

## Contract

Every `manifest.json` validates against
`fixtures/schema/scenario-manifest-v1.schema.json` and carries:

- stable scenario ID/schema/source commit/category/comparison class;
- platform/capability requirements;
- deterministic input/environment and referenced fixture files;
- ordered expected events plus state, persistence, process, and network
  observations;
- explicit normalization/security rules and one bounded timeout.

Every `result.*.json` validates against
`fixtures/schema/result-v1.schema.json` and carries:

- runner identity/version/platform;
- monotonically sequenced events;
- final state and persisted file/hash/mode evidence;
- process/network/log/redaction observations;
- duration category and classified result.

Canonical JSON is UTF-8, object keys sorted, compact separators, and one final
newline. Unknown schema fields fail validation so contract changes require a
new schema version.

## Comparison classes

| Class | Contract |
|---|---|
| `exact-output` | Canonical JSON/wire/schema/event sequence must match |
| `semantic-parity` | Named invariants match; platform/render wording may vary |
| `schema-compatibility` | Legacy records decode and preserve required/unknown data |
| `security-invariant` | Rust may be stricter but never weaker |
| `performance` | Compare controlled distributions/ceilings, not exact time |

## Normalization policy

Allowed only when declared:

- UUIDs/PIDs by encounter order;
- timestamps to relative/symbolic values;
- workspace/temp roots to `$WORKSPACE`/`$TMP`;
- path separators only where semantics permit;
- retry jitter to a permitted range.

Never normalized:

- event order or count;
- provider finish reason;
- permission/policy outcome;
- message/tool-call/result linkage;
- error/cancellation classification;
- hashes/redaction;
- duplicate, missing, or post-cancel events;
- escaped-root or surviving-process results.

The runner must reject normalization collisions: two distinct stable values
cannot collapse unless their rule explicitly declares encounter-order identity.

## Determinism and security

- No live provider calls or model-generated text.
- State roots and workspace are temporary and explicit.
- A synthetic secret canary is injected only into dedicated cases; raw canary
  text must be absent from canonical output.
- stdout is canonical output only for single-scenario capture; diagnostics go
  to sanitized stderr.
- Source commit mismatch, schema error, timeout, process leak, canary leak, or
  hash mismatch returns nonzero.
- Fixture hash index excludes itself and records SHA-256 over exact bytes.

## Commands, files, and validation status

Created:

- `fixtures/README.md`
- `fixtures/AGENTS.md`
- `fixtures/schema/scenario-manifest-v1.schema.json`
- `fixtures/schema/result-v1.schema.json`

Schema validation and end-to-end capture are performed by the Phase 5 oracle
and recorded in `python-oracle-report.md`.

## Unresolved issues and readiness

The charter is complete. Actual fixture coverage, repeatability, and canary
proof remain Phase 5 work. Cross-platform result producers must retain native
platform facts rather than normalizing them away.

