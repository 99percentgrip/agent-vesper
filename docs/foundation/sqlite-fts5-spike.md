# SQLite FTS5 Packaging Spike

Status: COMPLETE

## Objective

Determine whether the rebuildable session-search index can use reliable FTS5
across the required release targets and preserve Python search/security
semantics.

## Documentation and dependency evidence

Context7/current package metadata identifies rusqlite 0.40.1. Its relevant
features are:

- `bundled` → compile the packaged SQLite amalgamation and enable modern
  bindings;
- `bundled-full` → many unrelated extensions and is unnecessary for FTS5;
- `libsqlite3-sys` → link an available system SQLite.

The spike pins 0.40.1 and selects either `bundled-sqlite` (default) or
`system-sqlite`. Upstream currently uses edition 2024 and states a moving
latest-stable policy, so Vesper must pin it and test its own MSRV.

Local system evidence: Python/pkg-config SQLite 3.45.1 with
`ENABLE_FTS5`; no sqlite3 CLI is installed.

## Method

`spikes/sqlite-fts5/` implements only a disposable derived index:

- legacy-equivalent `indexed_sessions` and `messages_fts` schema with
  `unicode61`;
- WAL and 5-second busy timeout;
- transactional replacement per session;
- system-message exclusion;
- 32K message cap and synthetic credential redaction;
- browse newest-first and BM25-ranked user/assistant term search;
- empty result on index failure and explicit delete/rebuild after corruption.

Tests also prove lock contention resolves within the busy timeout and that
`unicode61` matches `cafe` to `Café`.

## Commands and results

- Context7 rusqlite query; `cargo info rusqlite --verbose`;
- local Python SQLite version/compile-option probe and pkg-config probe;
- `cargo test --locked`: **6 passed, 0 failed** using bundled SQLite;
- `cargo test --locked --no-default-features --features system-sqlite`:
  **6 passed, 0 failed** using system SQLite;
- feature-tree and test-binary size inspection.

Local debug test binaries were approximately:

- bundled: 14,778,472 bytes;
- system: 11,342,112 bytes.

That ~3.4 MiB debug difference is only packaging direction, not a release-size
claim.

## Verdict

**Bundled rusqlite is the recommended default for release artifacts, locally
validated; system SQLite may remain a developer/distribution override.**

Bundling gives deterministic FTS5 availability and tokenizer/version behavior.
Runtime startup must still execute both:

```sql
SELECT sqlite_compileoption_used('ENABLE_FTS5');
CREATE VIRTUAL TABLE ... USING fts5(...);
```

Failure remains nonfatal because session JSON is authoritative. The index may
be quarantined/deleted and rebuilt; it must never block session save/load.

## Five-target CI-ready matrix

| Target family | Cargo mode | Required checks | Current state |
|---|---|---|---|
| Linux x86-64 | bundled + optional system | all 6 tests, release binary size/link inspection | Bundled/system locally validated |
| Linux ARM64 | bundled | compile/run all 6 on native/emulated runner; inspect FTS option | CI pending |
| macOS Intel | bundled | compile/run, WAL/temp behavior, package size | CI pending |
| macOS Apple Silicon | bundled | compile/run, WAL/temp behavior, package size | CI pending |
| Windows x86-64 | bundled | compile/run, locking/rename/ACL-adjacent behavior, package size | CI pending |

The matrix is implemented in the foundation workflow created with the process
spike. No non-Linux row is marked validated until it runs.

## Files created

- `spikes/sqlite-fts5/{AGENTS.md,README.md,Cargo.toml,Cargo.lock}`
- `spikes/sqlite-fts5/src/lib.rs`
- this report

## Unresolved issues and readiness

Release-profile binary/package size, MSRV 1.88, cross-compilation toolchains,
and all non-local target runs remain CI validation pending. Local FTS5,
ranking, redaction, WAL/busy timeout, corruption recovery, and fail-soft
assumptions are resolved for workspace foundation.

