# Stage 5 Part 5 Disk Invariance Proof

Status: COMPLETE

## Scope

This report covers the real production path:

```text
agent-vesper-acp process
  → ACP protocol-v1 stdio
  → vesper-acp
  → vesper-runtime
  → vesper-sessions
  → synthetic Agent Vesper and Native GLM ACP roots
```

No live provider request, real user state, persistent writer, repair, migration,
SQLite, or FTS operation was used.

## Proof method

Each process vector creates isolated synthetic Agent Vesper and legacy roots
containing nine files. Immediately before spawning the real executable, the
test records for every file:

- its root-qualified relative name;
- SHA-256 digest;
- byte length;
- modification timestamp when reported by the host filesystem.

After ACP stdin reaches EOF and the process exits successfully, the same
snapshot is recomputed and compared with exact `BTreeMap` equality. This proves
file-set equality, content-hash equality, byte-length equality, and timestamp
equality together. The test additionally asserts that the isolated
`XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_DATA_HOME`, and `XDG_STATE_HOME`
directories were not created by the application.

## Results

| Process vector | Persistent files | Before/after equality | New files | Timestamp changes | Result |
| --- | ---: | --- | ---: | ---: | --- |
| Listing | 9 | Exact | 0 | 0 | PASS |
| Legacy minimal load | 9 | Exact | 0 | 0 | PASS |
| Resume | 9 | Exact | 0 | 0 | PASS |
| Unknown-field load | 9 | Exact | 0 | 0 | PASS |
| Missing-metadata JSON fallback | 9 | Exact | 0 | 0 | PASS |
| Fork | 9 | Exact | 0 | 0 | PASS |
| Close | 9 | Exact | 0 | 0 | PASS |
| Agent Vesper/legacy ID collision | 9 | Exact | 0 | 0 | PASS |
| Corrupt record | 9 | Exact | 0 | 0 | PASS |
| Unsupported version | 9 | Exact | 0 | 0 | PASS |
| Raw-secret-bearing Agent Vesper record | 9 | Exact | 0 | 0 | PASS |

The suite performed 99 exact file-state comparisons across 11 independent
process executions (198 SHA-256 computations across the before/after
snapshots). All comparisons passed.

## Security observations

- Agent Vesper wins a cross-source ID collision; the legacy duplicate is not
  replayed.
- Replay contains only supported visible user/assistant text. System prompts,
  provider reasoning, and tool internals are absent.
- Raw secret-bearing provider configuration is rejected before runtime
  adoption. The canary is absent from ACP stdout and stderr.
- Corrupt and unsupported records return bounded typed errors. A subsequent
  list request succeeds, proving the ACP dispatcher remains usable.
- Corrupt records are not repaired and unsupported records are not rewritten.
- Fork and close mutate only in-memory actors.
- Missing metadata uses bounded JSON fallback without creating a sidecar.
- All process stdout remains parseable ACP JSON-RPC, and the test harness reaps
  every child.

## Concurrency and consistency evidence

- Per-session keyed load gates serialize reads/adoption for one session ID
  without a global session-state or filesystem lock.
- Concurrent load and resume of the same ID return the same single actor.
- Separate session IDs and listing can progress concurrently through the
  bounded filesystem semaphore.
- A delayed persistent read rechecks the actor registry before adoption, so a
  newer in-memory session cannot be overwritten by stale disk state.
- Atomic file replacement during repeated reads yields only loaded, corrupt,
  or missing typed outcomes; it never repairs or writes a record.

## Commands

- `cargo test -p vesper-domain -p vesper-sessions -p vesper-runtime`
- `cargo test -p agent-vesper-acp --test process_blockers --all-features
  persistence_vectors -- --nocapture`
- `cargo test -p agent-vesper-acp --test process_blockers --all-features --
  --nocapture`
- `cargo test -p agent-vesper-acp --tests --all-features`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`

All final invocations passed locally on Linux x86-64. Other target families
remain CI-validation pending.
