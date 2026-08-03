# vesper-checkpoints — workspace snapshots, rollback, and session lineage

## Purpose

Own the **explicit-only workspace checkpoint and rollback subsystem**
(ADR 0012 — Stage 14): file-state snapshots, restore, session/lineage
tracking, plus the bounded cron / export / clipboard / CI surface that
groups with it. Backs the Tier C Phase 9 un-stubbed commands
(`/sessions-new`, `/sessions`, `/lineage`, `/branch`, `/rename`,
`/checkpoint`, `/rollback`, `/rewind`, `/undo`, `/loop`, `/export`,
`/copy`, `/ci`).

The crucial historical mandate: the original Python oracle suffered an
uncontrolled file-descriptor leak (`Errno 24: Too many open files`) caused
by unmanaged SQLite transactions leaving `.wal`/`.shm` files hanging. This
crate bypasses SQLite entirely and relies on strict Rust RAII (`Drop`) so
every opened file handle is returned to the OS the moment its owning scope
exits — no `@contextmanager` wrapper needed.

## Ownership

- `src/lib.rs` — public re-exports and crate-level docs.
- `src/error.rs` — `CheckpointError` (sanitized; never leaks paths or
  payloads that may carry secrets).
- `src/io.rs` — atomic write helpers + JSONL reader (mirrors
  `vesper-memory`).
- `src/types.rs` — `CheckpointRecord`, `CheckpointKind`, `FileSnapshot`,
  `SessionRecord`, `SessionStatus`, `CronEntry`, plus bound constants.
- `src/ledger.rs` — `CheckpointsLedger` (append-only JSONL lineage log)
  with append / list / prune / forget.
- `src/snapshot.rs` — workspace snapshot (copy files into
  `checkpoints/<id>/`) and `restore()` (copy files back to the workspace).
- `src/sessions.rs` — `SessionLineage` (sessions-new / sessions / lineage
  / branch / rename).
- `src/cron.rs` — `CronRegistry` (`/loop`): records, updates, pauses,
  resumes, removes, and leases bounded entries; the shared harness owns the
  optional host scheduler that executes due jobs and records bounded results.
- `src/export.rs` — `SessionExporter` (`/export`, `/export last`): writes
  either the full transcript + lineage or only the final assistant response
  to a bounded markdown file.
- `src/clipboard.rs` — `ClipboardPort` (`/copy`): abstraction with a
  safe fallback when no platform clipboard is reachable from this
  terminal.
- `src/ci.rs` — `CiStatusReader` (`/ci`): shells out to `gh` if present,
  otherwise returns a clear "unavailable" status.

## Local Contracts

- Depends only on `vesper-domain` and `vesper-security`. No provider,
  runtime, ACP, sessions, agent, testkit, SQLite, HTTP, or TUI dependency.
- **No SQLite, no git refs.** Storage is plain JSONL + plain file copies
  under a configurable root. The user's native git repository is never
  touched.
- **Explicit-only checkpoints.** Nothing snapshots automatically on agent
  turns or file mutations. A checkpoint exists only when the driver (or
  the harness on a major session transition) explicitly requests one via
  `CheckpointsLedger::create`.
- **Strict RAII.** No `File` is ever stored in a long-lived struct. Every
  read / write opens, operates, and drops inside a tight scope so the OS
  reclaims the descriptor immediately. There are no `lazy_static` /
  `once_cell` file handles, no background `select()`-style loops holding
  descriptors, and no `Arc<Mutex<File>>`.
- **Hard bounds** (configurable but capped by `HARD_*` constants):
  - `MAX_FILE_SIZE_BYTES` per individual file (default 1 MiB, hard 25 MiB)
  - `MAX_FILES_PER_CHECKPOINT` (default 1000, hard 20 000)
  - `MAX_CHECKPOINT_SIZE_BYTES` total per checkpoint (default 10 MiB)
  - `MAX_RETENTION_COUNT` checkpoints kept on disk (default 50, hard 100)
  - `MAX_LINEAGE_DEPTH` session chain length (default 100)
  - `MAX_CRON_JOBS` (default 500)
- Cron records remain JSONL-compatible with older entries; missing `enabled`
  fields default to `true`.
- **Pruning is aggressive and synchronous.** Every `create` checks the
  retention count and unlinks the oldest `checkpoints/<id>/` directories
  before the new checkpoint is committed.
- **Sensitive-file guard.** Snapshots refuse to copy files matching the
  oracle's `_SENSITIVE` set (`.env`, `credentials.json`, `id_rsa`,
  `id_ed25519`, `*.key`, `*.pem`, `*.p12`, `*.pfx`).
- **Ignored-tree guard.** Snapshots refuse to descend into `.git`,
  `.venv`, `venv`, `node_modules`, `dist`, `build`, `__pycache__`,
  `target`, `.agent-vesper`.
- Stores never create the root directory; the composition boundary
  (binary) is responsible for ensuring it exists.
- No live provider calls, no network I/O (the CI reader is a bounded
  subprocess invocation of `gh`, never a direct API call).

## Work Guidance

- When adding a new snapshot/restore path, route through
  `snapshot::copy_file_into` / `snapshot::restore_file_from` so the
  sensitive-file and ignored-tree guards stay central.
- When raising errors, use `CheckpointError::io(kind)` instead of `?` on
  raw `io::Result` so the diagnostic stays secret-safe.
- The JSONL log is the source of truth for checkpoint records; the
  `checkpoints/<id>/` directory is the file payload. Both must be
  rewritten together: append the line first, then write the payload
  directory, then on the next prune the orphan directory is swept.
- Keep `ClipboardPort` and `CiStatusReader` deliberately minimal — they
  are real implementations of the un-stubbed commands but their platform
  surface is intentionally narrow.

## Verification

- `cargo test -p vesper-checkpoints` — unit + integration tests:
  file-state restoration round-trips, `MAX_RETENTION_COUNT` unlinking,
  RAII discipline (implicit via Rust but asserted via scoped helpers),
  bounds enforcement, sensitive-file refusal, ignored-tree refusal,
  session lineage tracking, cron registry, export writer, clipboard
  fallback, CI-status fallback.
- `cargo xtask architecture` — confirms the new crate satisfies the
  production dependency allowlist and the source-tree unsafe ban.

## Child DOX Index

No children.
