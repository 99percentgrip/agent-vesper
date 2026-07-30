# Stage 5 Read-Only Session Store Report

Status: COMPLETE

## Objective

Establish bounded, read-only session discovery, compatibility decoding,
runtime adoption, and ACP replay without exposing any persistent writer,
repair path, migration path, SQLite index, or user-state mutation.

## Implemented surface

- `vesper-sessions::SessionReader`, `SessionLister`, and `SessionRepository`
  expose list/load/resume/replay reads.
- `SessionRepositoryCapabilities::read_only` marks write, delete, migrate, and
  persistent search unsupported; `reject` returns a typed failure.
- `FilesystemSessionStore` performs bounded, non-recursive reads behind an
  explicit blocking-I/O semaphore.
- `CompositeSessionRepository` resolves collisions in fixed order: in-memory,
  Agent Vesper, then legacy Native GLM ACP.
- `LegacySessionDecoder` and `VesperSessionDecoder` return typed missing,
  corrupt, unsupported-version, bounds, permission, and unsafe-path outcomes.
- `LegacyRuntimeConverter` creates provider-neutral, inspectable runtime state
  without executing persisted plans, tools, goals, memory, or checkpoints.
- `ReplayPlan` delivers ordered safe updates through an acknowledgement-based
  sink before lifecycle completion can be sent.

## Safety bounds

The production defaults include:

| Surface | Bound |
| --- | ---: |
| Session record | 16 MiB |
| Metadata sidecar | 64 KiB |
| Directory entries | configurable, bounded before enumeration proceeds |
| Session ID | 256 bytes |
| Messages | 10,000 |
| Content/reasoning value | 1 MiB |
| Additional roots | 128 |
| Plan items | 1,000 |
| JSON compatibility depth | 64 |
| Unknown fields | 256 top-level fields; 1 MiB measured data |

The byte bound is checked before full record allocation. Requested IDs are
mapped through `SessionFileName`; they are never treated as paths. Symlinks that
resolve outside a configured root are rejected.

## Fixture and process evidence

- Seven authoritative session scenarios exercise schema-v1 compatibility,
  omitted fields, unknown fields, corruption, lineage, replay, and reasoning
  retention.
- Eleven production-process persistence vectors exercise list, load, resume,
  metadata fallback, replay visibility, fork, close, corrupt/unsupported
  outcomes, collisions, and concurrent loads.
- Each process vector compares the complete synthetic store file set, SHA-256,
  length, and modification timestamp before and after execution.
- `vesper-testkit` provides temporary legacy/Agent Vesper read-store builders,
  corrupt/truncated record builders, a session fixture loader, a complete
  file-tree hash manifest, and a no-write assertion.

## Governance

- `cargo xtask sessions verify` validates Stage 5 coverage and runs the session
  and testkit suites.
- `cargo xtask architecture` rejects runtime/ACP/GLM dependencies, SQLite,
  filesystem mutation calls, session writer APIs, and directory creation from
  production session sources.
- Cargo Deny separately bans `rusqlite`, `sqlx`, and `libsqlite3-sys` during
  this read-only stage.
- The fixture index excludes coverage maps and remains the authoritative
  154-payload hash set.

## Dependency result

Stage 5 Part 6 adds no third-party package. Testkit helpers reuse pinned
`serde_json`, `sha2`, and `thiserror`. No SQLite/FTS dependency exists.

## Verification

Final local results:

- 151 workspace tests passed on Rust 1.95 and Rust 1.88.
- Formatting, strict Clippy, workspace checks, and doc tests passed.
- All fixture, contract, provider, runtime, ACP, session, architecture, and
  aggregate xtask commands passed.
- Cargo Audit found no vulnerable dependency; Cargo Deny passed advisories,
  bans, licenses, and sources with three reviewed duplicate-version warnings.
- Ten same-ID load repetitions, ten atomic-replacement repetitions, and five
  complete persistence-process repetitions passed with zero flakes.
- Linux x86-64 is locally validated; the remaining four target families are CI
  validation pending.

## Deferred

- Transactional Agent Vesper writes and revisions
- Crash recovery and atomic replacement
- Derived metadata generation
- Explicit migration/repair
- Persistent search and any later SQLite/FTS design
