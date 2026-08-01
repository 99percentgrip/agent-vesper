# ADR 0012: Stage 14 — Workspace Checkpoints, Session Lineage, and Bounded Cron/Export/Clipboard/CI

Status: ACCEPTED

Builds on: [ADR 0011](0011-stage-12-persistent-memory-and-awareness.md)
(the `Deferred { command, reason }` registry variant).

## Context

ADR 0011 un-stubbed 13 awareness/memory commands. The lead architect then
issued Stage 14: un-stub the worktree / checkpoint / rollback / undo /
session-lineage / cron / export / clipboard / CI command surface — 13
more commands.

### Crucial historical mandate: Errno 24 prevention

The original Python oracle suffered an uncontrolled file-descriptor leak
(`Errno 24: Too many open files`) caused by unmanaged SQLite transactions
leaving `.wal` / `.shm` files hanging. The fix in Python was a strict
`@contextmanager` lifecycle wrapper. **This ADR bypasses SQLite entirely**
and relies on Rust's RAII (`Drop`) so every opened file handle is returned
to the OS the moment its owning scope exits — no wrapper needed.

### Oracle data model (audit)

The frozen Python oracle at `bf4d4287` ships:

- `glm_acp/checkpoints.py` (843 LOC): `CheckpointManager` with
  `CheckpointLimits` + `CheckpointStoragePolicy`. Storage uses compressed
  loose Git blob objects in a private shadow database. Bounds:
  `DEFAULT_MAX_FILE_MIB = 25`, `HARD_PROJECT_HISTORY = 100`,
  `DEFAULT_AUTO_CHECKPOINT = False`. Sensitive-file guard:
  `_SENSITIVE_NAMES = {.env, credentials.json, id_rsa, id_ed25519}`,
  `_SENSITIVE_SUFFIXES = {.key, .pem, .p12, .pfx}`. Ignored-tree guard:
  `_IGNORED = {.git, .venv, venv, node_modules, dist, build, __pycache__}`.
- `glm_acp/cron.py` (544 LOC): `MAX_JOBS = 500`, `MAX_PROMPT_CHARS =
  32_000`, JSON-backed job registry.

### Commands this ADR un-stubs

Thirteen commands move from `Deferred` to a real, persistent backing:

| Command | Op | Backing |
|---|---|---|
| `/sessions-new [name]` | `SessionCreate` | `SessionLineage::create` |
| `/sessions` | `SessionList` | `SessionLineage::list` |
| `/lineage` | `LineageShow` | `SessionLineage::lineage` |
| `/branch [name]` | `SessionBranch` | `SessionLineage::branch` |
| `/rename <name>` | `SessionRename` | `SessionLineage::rename` |
| `/checkpoint [label]` | `CheckpointCreate` | `CheckpointsLedger::create` |
| `/rollback <id>` | `CheckpointRollback` | `CheckpointsLedger::restore` |
| `/rewind <id>` | `CheckpointRewind` | `CheckpointsLedger::restore` (alias) |
| `/undo [N]` | `CheckpointUndo` | `CheckpointsLedger::recent` + `restore` |
| `/loop <prompt>` | `CronRegister` | `CronRegistry::register` |
| `/export` | `SessionExport` | `SessionExporter::export` |
| `/copy [target]` | `ClipboardCopy` | `ClipboardPort::copy` |
| `/ci` | `CiStatus` | `CiStatusReader::status` |

## Decision

1. **New crate `vesper-checkpoints`** owns the workspace snapshot / rollback
   / session-lineage / cron / export / clipboard / CI surface. It depends
   only on `vesper-domain` and `vesper-security` (no provider, runtime,
   ACP, sessions, agent, testkit, SQLite, HTTP, or TUI dependency).

2. **No SQLite, no git refs.** Storage is plain JSONL + plain file copies
   under a configurable root. The user's native git repository is never
   touched (the oracle's `refs/vesper/` shadow-database approach is
   deliberately NOT mirrored — it would pollute the user's repo).

3. **Strict RAII / Drop semantics.** Every `File` opened in this crate is
   scoped to a function body and dropped at the closing brace. No `File`
   is stored in a long-lived struct; there are no `lazy_static` /
   `once_cell` file handles; there are no background loops holding
   descriptors. The OS reclaims every descriptor the moment its scope
   exits, regardless of how many snapshots are taken. **This is the
   architectural guarantee that the Errno 24 leak cannot recur.**

4. **Explicit-only checkpoints.** Nothing snapshots automatically on agent
   turns or file mutations. A checkpoint exists only when the driver (or
   the harness on a major session transition) explicitly requests one via
   `CheckpointsLedger::create`. The directive's "no aggressive
   auto-snapshotting" mandate is enforced structurally — there is no
   auto-snapshot code path at all.

5. **Storage layout** under one configurable root directory:
   - `checkpoints.jsonl` — append-only `CheckpointRecord` log.
   - `checkpoints/<id>/` — payload directory holding the copied files for
     checkpoint `<id>`. Unlinked by `CheckpointsLedger::prune` when
     `MAX_RETENTION_COUNT` is exceeded.
   - `sessions.jsonl` — append-only `SessionRecord` lineage log.
   - `cron.jsonl` — append-only `CronEntry` registry.
   - `exports/<timestamp>.md` — bounded markdown session exports.
   - `clipboard.log` — append-only log of clipboard targets.

   All writes are atomic (write-to-temp + `fsync` + rename), confined to
   the absolute root, and bounded by configured byte limits — the same
   discipline as the Stage 6 session writer and the Stage 12 memory
   writer.

6. **Hard bounds** (defaults; the composition boundary may lower but not
   raise past `HARD_*`):
   - `MAX_FILE_SIZE_BYTES` per individual file (default 1 MiB, hard 25 MiB)
   - `MAX_FILES_PER_CHECKPOINT` (default 1000, hard 20 000)
   - `MAX_CHECKPOINT_SIZE_BYTES` total per checkpoint (default 10 MiB, hard 250 MiB)
   - `MAX_RETENTION_COUNT` checkpoints kept on disk (default 50, hard 100)
   - `MAX_LINEAGE_DEPTH` session chain length (default 100)
   - `MAX_CRON_JOBS` (default 500)

7. **Pruning is aggressive and synchronous.** Every `CheckpointsLedger::create`
   checks the retention count and unlinks the oldest `checkpoints/<id>/`
   directories BEFORE the new checkpoint is committed. The dedicated test
   `prune_unlinks_payload_directories_when_retention_cap_exceeded` proves
   this end-to-end: it creates `MAX_RETENTION_COUNT + 5` checkpoints and
   asserts both that the oldest 5 are gone from the ledger AND that their
   payload directories are unlinked from disk.

8. **Sensitive-file guard + ignored-tree guard.** Snapshots refuse to copy
   `.env`, `credentials.json`, `id_rsa`, `id_ed25519`, `*.key`, `*.pem`,
   `*.p12`, `*.pfx`, and refuse to descend into `.git`, `.venv`, `venv`,
   `node_modules`, `dist`, `build`, `__pycache__`, `target`,
   `.agent-vesper`, `.glm-acp`. Mirrors the oracle's `checkpoints._SENSITIVE_*`
   and `checkpoints._IGNORED`.

9. **SHA-256 integrity check on restore.** Every restored file is
   re-hashed and skipped (not failed) if the on-disk bytes do not match
   the recorded SHA-256. A torn checkpoint cannot brick the workspace.

10. **TUI wiring** follows the existing `pending_*` drain pattern:
    - The resolver returns `CommandOutcome::Checkpoint(CheckpointOp)`.
    - `dispatch` records `SessionState.pending_checkpoint_op:
      Option<CheckpointOp>`.
    - The binary owns a `CheckpointStores` bundle (`CheckpointsLedger` +
      `SessionLineage` + `CronRegistry` + `SessionExporter` +
      `ClipboardPort`; `CiStatusReader` is process-scoped) and drains the
      op synchronously after dispatch (these are local filesystem
      reads/writes — fast enough not to block the UI; `/ci` shells out to
      `gh` if present).

11. **Composition boundary root**: `AGENT_VESPER_CHECKPOINT_ROOT` env
    var, with a fallback to `.agent-vesper/checkpoints/` under the
    current directory. If any store cannot be opened, that store stays
    `None` and checkpoint commands surface a clear "store unavailable"
    notice rather than crashing the TUI.

12. **The `Deferred` variant stays** for the remaining ~28 commands
    (composer features needing a TUI rebuild, ratatui UI rebuilds,
    live-session settings needing a live provider API, image subsystem,
    audio, mobile, plus Stage 15's `mcp` + `plugins`). Those subsystems
    ship in subsequent ADRs.

## Consequences

- **Positive**: 13 more commands move from stubbed to production-functional.
  The agent can now snapshot the workspace, roll back file mutations,
  track session lineage, register cron entries, export the session,
  copy to the clipboard, and surface CI status — all backed by durable,
  RAII-safe storage.
- **Positive**: the Errno 24 leak class is structurally impossible. Every
  `File` is scoped; there is no path by which a file descriptor can be
  held past its function body. The architect's specific concern from
  `sqlite_leak_fix.png` is closed at the language level.
- **Positive**: the snapshot/restore path is integrity-checked (SHA-256
  per file) so a torn checkpoint degrades gracefully rather than bricking
  the workspace.
- **Trade-off**: snapshots copy files verbatim (not content-addressed
  diffs). For workspaces with many large files this is more expensive
  than the oracle's git-blob approach, but it is dramatically simpler,
  avoids polluting the user's git repo, and stays within the bounded
  contract (≤ 10 MiB per checkpoint, ≤ 50 retained).
- **Trade-off**: `/loop` records cron entries but does not run a
  scheduler — the TUI binary is not a daemon. A future long-running
  process can read the registry and fire the prompts.
- **Trade-off**: `/copy` falls back to a persistence strategy
  (`<root>/clipboard.log`) when no native clipboard is reachable from
  the terminal. The fallback is honest — the value is recorded and
  retrievable, but the user is told the native clipboard did not fire.
- **Trade-off**: `/ci` shells out to `gh` (a bounded subprocess
  invocation, never a direct API call). When `gh` is missing the command
  surfaces a clear "unavailable" notice.

## Verification

- `cargo test -p vesper-checkpoints` — 30 unit/integration tests covering
  file-state restoration round-trips, **MAX_RETENTION_COUNT unlinking of
  payload directories from disk** (the lead architect's specific demand),
  SHA-256 mismatch handling, path-escape refusal, bounds enforcement,
  sensitive-file refusal, ignored-tree refusal, session lineage tracking,
  cron registry persistence, export writer bounds, clipboard fallback,
  CI-status fallback.
- `cargo test -p agent-vesper-tui --lib` — 7 new Phase 9 tests proving
  every one of the 13 commands resolves to a real `Checkpoint(CheckpointOp)`
  outcome, plus the `pending_checkpoint_op` drain records correctly and
  argument-required commands reject empty arguments with usage errors.
- `cargo xtask architecture` — 17 packages validated (was 16).
- `cargo xtask verify` — 514 tests pass, 0 failures (was 477).
- `phase7_every_deferred_command_names_a_subsystem` updated to assert the
  remaining 28 deferred commands still resolve to `Deferred` — the 13
  Stage 14 commands are no longer in that list.

## Migration matrix (Stage 14 progress)

| Category | Shipped in this ADR | Still deferred |
|---|---|---|
| Memory subsystem (13) | (shipped ADR 0011) | — |
| Worktree & checkpoints (9) | sessions-new, sessions, lineage, branch, rename, checkpoint, rollback, rewind, undo | — |
| Cron/loop (1) | loop | — |
| Export/clipboard (2) | export, copy | — |
| CI integration (1) | ci | — |
| MCP & plugins (2) | — | mcp, plugins |
| Composer (6) | — | history, search, prompt, btw, blocks, annotate |
| ratatui UI rebuild (8) | — | theme, vim, keybinds, statusline, screen-reader, native-mouse, reasoning-panel, toggle-thinking |
| Live session settings (6) | — | settings, permission, mode, generation, auxiliary, mixture |
| Image subsystem (4) | — | image, attach, image-render, screenshot |
| Mobile/sound (2) | — | mobile, sound |
| **TOTAL** | **26 un-stubbed (13 + 13)** | **28 still deferred (Stages 15 + exclusions)** |
