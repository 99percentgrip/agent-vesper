# Persistence and Compatibility

Status: COMPLETE

## Scope

This report identifies authoritative and derived stores, schemas, readers/writers, failure semantics, concurrency, redaction, and Rust compatibility obligations. Evidence paths refer to the frozen source.

## Root/profile resolution

Global config uses `GLM_ACP_CONFIG_DIR` when set; otherwise Windows `%APPDATA%/glm-acp`, macOS `~/Library/Application Support/glm-acp`, and Linux/XDG config conventions, then applies the active profile (`config.py:685-710`; `profiles.py:13-24`). Profile names are validated against traversal. Sessions are a historical exception: default stays `~/.glm-acp/sessions`, named profiles use `~/.glm-acp/profiles/<profile>/sessions` (`session_store.py:24-69`).

**Compatibility rule:** Rust must implement both historical root schemes. A new Vesper root may be introduced only with an import/discovery layer and explicit user choice; never silently move/delete Python state.

## Session JSON schema 1

`Session.to_dict` writes `version: 1` plus:

- cwd;
- GLM model/thought/mode/API endpoint/generation/auxiliary/mixture settings;
- title, parent/root lineage, permission mode;
- plan and messages;
- cumulative input/output/cache and estimated/context-pressure data;
- task context, compaction proposals/quality, instruction targets;
- nested verification, awareness, metacognition, deliberation, repository-intelligence, meta-learning state;
- goal/subgoals/pause/turn budget;
- ordered loaded tool names and last checkpoint ID.

Evidence: `agent.py:555-600`. Runtime locks, clients, caches, background tasks, active checkpoint ID, scheduled-run flag, context-size derivation, and sandbox instance are intentionally not persisted (`agent.py:377-447`).

`from_dict` defaults missing legacy fields, bounds variable lists/strings, rejects invalid auxiliary vision/model combinations, recalculates context size, and rebuilds the managed system prompt (`agent.py:603-671`). It does not reject unknown fields.

### Files and atomicity

- `<session-id>.json`: authoritative state, atomic `.tmp` replace, 0600.
- `<session-id>.meta`: derived list metadata, atomic `.meta.tmp` replace, 0600.
- `session-index.sqlite3`: derived WAL/FTS index, 0600.

Evidence: `session_store.py:71-113`, `:193-229`.

### Corruption/concurrency

JSON load corruption logs and returns absent (`:233-245`). Listing skips corrupt sidecars/files and backfills legacy JSON without sidecars (`:247-285`). There is no explicit cross-process session-file lock or revision; concurrent writers can last-write-win. Rust must add optimistic revision or an advisory lock while maintaining single-process actor serialization.

### Reasoning/privacy

Exact `reasoning_content` is removed on serialization only when `GLM_ACP_PERSIST_REASONING` is false; default is true (`config.py:669-682`; `agent.py:557-562`). Preserve the reader and make any new default a product/privacy decision.

## Session SQLite index

Tables:

```sql
indexed_sessions(session_id PRIMARY KEY, cwd NOT NULL, title, updated_at)
messages_fts USING fts5(session_id UNINDEXED, ordinal UNINDEXED, role UNINDEXED,
                        content, tokenize='unicode61')
```

Evidence: `session_store.py:83-110`.

Indexing excludes system messages, extracts only text blocks, caps 32K per message, and redacts private keys, Bearer tokens, and common credential assignments (`:29-39`, `:115-164`). Search:

- bounds limit/window 1–20;
- browsing returns newest sessions;
- term search extracts at most 12 word/hyphen tokens, combines exact terms with AND, restricts hits to user/assistant, ranks BM25, returns one hit per session with surrounding messages;
- any SQLite/value failure returns an empty result.

Evidence: `session_store.py:306-413`.

**Compatibility:** Treat the DB as disposable. Rust may rebuild it from JSON and should not require byte/schema compatibility for its own new index, but must read existing results or rebuild without altering source. Exact query semantics are semantic-parity fixtures.

## Configuration/preference stores

| File | Schema/fields | Behavior |
|---|---|---|
| `credentials.json` | object containing API key | environment precedence; atomic 0600; missing/malformed means absent (`config.py:711-775`) |
| `max-iterations.json` | schema 1, integer `value` 1–1000 | env wins; malformed fallback to 50; atomic/fsync/private (`config.py:48-134`) |
| `statusline.json` | schema 1/segment list | unknown/empty/malformed shows all (`config.py:137-205`) |
| `theme.json` | schema 1/theme | bounded supported theme; fallback none (`config.py:208`, `:352-395`) |
| `screen-reader.json` | schema 1/enabled | fallback false (`config.py:213`, `:309-350`) |
| `vim.json` | schema 1/enabled | fallback false (`config.py:218-253`) |
| `keybinds.json` | schema 1/bindings | action/key validation; malformed means defaults (`config.py:255-307`) |
| `mcp.json` | server map | load ignores malformed; current save/remove uses direct `write_text`, not atomic/private hardening (`mcp.py:169-229`) |
| `hooks.json` | user-authored hooks | invalid file/entries ignored; executable pins checked at use (`hooks.py:27-77`) |

Rust should retain filenames/readers during parity, strengthen `mcp.json` writes atomically, and store provider credentials in namespaced records or OS keyring handles without removing the legacy GLM key.

## Project-local knowledge

| Store | Format/version | Writer/reader and constraints |
|---|---|---|
| `.glm-acp/memory.md` | Markdown entries | atomic; contained; secret/promptware scan; exact append/forget/batch (`memory.py:286-409`) |
| `.glm-acp/skills/<slug>/SKILL.md` | frontmatter + Markdown | agent-owned paths, safe slugs, bounded content, environment/tool/task metadata (`memory.py:496-731`) |
| `.glm-acp/skills/.usage.json` | version 1; timestamps/use/revision/state/pin data | malformed fallback; atomic (`memory.py:518-556`) |
| `.glm-acp/skills/.bundles.json` | version 1; named skill sets/instruction/hash metadata | atomic; tamper/promptware checks (`memory.py:876-986`) |
| `.glm-acp/skills/.archive/*` | moved skill directories | explicit archive/restore; no auto-delete (`memory.py:732-875`) |
| `.glm-acp/skills/.candidates/*.draft.json` / `*.json` | version 1 evaluation evidence | drafts inert; candidate promotion explicit after gate (`memory.py:1042-1222`) |
| `.glm-acp/evaluation/failure-cases.json` | project benchmark catalog | explicit failure-draft promotion only (`failure_corpus.py:157-210`) |

**Compatibility:** Vesper must read and preserve these project-local formats because they travel with repositories. Writes should either remain byte-compatible through parity or use a new version with atomic migration and rollback.

## Private learning/observability

- `user.md`: approved profile preferences, private, exact forgetting (`memory.py:423-495`).
- `trajectory.jsonl`: schema-1 metadata events; append-only 0600 (`telemetry.py:22-97`). Observability reads a bounded tail and ignores malformed/non-schema lines (`observability.py:33-57`).
- `failure-corpus/drafts.jsonl`: schema-1 private append; hashed project identity and coarse file suffixes; discard rewrites atomically (`failure_corpus.py:43-141`).
- Capability profiles are computed from trajectory events, not a separate raw task store (`metacognition.py:165-242`).
- Awareness/deliberation/repository/meta-learning state is embedded in sessions, not separately global.

Rust may introduce a versioned event envelope but must retain readers for schema 1. Never enrich legacy metadata with task bodies/paths.

## Checkpoints

Paths under profile config:

- `checkpoint-auto.json`, `checkpoint-limits.json`, `checkpoint-storage.json` — schema 1.
- `checkpoints/store/objects/<prefix>/<oid>` — zlib-compressed Git-compatible blob bytes.
- `checkpoints/workspaces/<workspace-hash>/<checkpoint>.json` — schema-2 manifests.
- legacy schema-1 full-copy directories with `manifest.json`.

Evidence: `checkpoints.py:26-55`, `:142-164`, `:285-540`.

Manifest records relative path, baseline hash/object/mode and later agent-produced hash. Lock-directory owner metadata coordinates store mutation. Creation is bounded and excludes secrets/ignored/large content. Retention limits global bytes, per-project history and age; GC deletes only unreferenced objects. Rollback preflights all conflicts and attempts transactional restoration (`checkpoints.py:570-850`).

**Compatibility:** `vesper-checkpoints` must read schemas 1 and 2 and use the same Git object identity/compression. A schema-3 writer is allowed only after differential create/list/rollback/migrate/GC tests. Never modify repository `.git`.

## Cron/automation

- `cron/jobs.json`: version 1 `{version,jobs}` under an OS file lock; atomic, fsync, 0600 (`cron.py:44-147`).
- Job records include ID/name/prompt, parsed schedule/display/timezone, workspace/workdir, skills/bundles/script/no-agent, enabled/state/repeat/run counts, timestamps, claim, bounded history, origin session (`cron.py:271-343`).
- `cron/results/<job-id>/<UTC stamp>-<token>.json`: bounded/redacted output/error (`cron.py:623-643`).
- `.jobs.lock` and `daemon-heartbeat` are coordination files.

Claims are token-owned with TTL and renewal; stale recurring claims recover without duplicating missed slots, stale one-shot claims recover once (`cron.py:487-620`). Store corruption/version mismatch fails closed. Rust must preserve schedule interpretation, claim ownership, and version-1 reader before taking over daemon duties.

## Plugins, trust, workers, worktrees

- `plugins/<id>/plugin.json`, `manifest.sha256`, data files: schema 1, hash-pinned, atomic directory swap (`plugins.py:134-425`).
- `trusted-plugin-publishers.json`: schema 1 publisher→Ed25519 public key, private atomic file (`plugins.py:190-230`).
- Signing key JSON schema 1 (`algorithm=ed25519`) and public key JSON; private 0600/public 0644 (`plugins.py:33-82`).
- `workers/<session>.jsonl`: bounded private worker transcripts (`agent.py:4102-4135`, `:4272`).
- `worktrees/`: manager-owned registry/detached checkout area under profile config (`worktrees.py:21-239`).

Rust must validate existing plugin hashes/signatures exactly. Worker transcripts may be treated as diagnostics, but cleanup/retention and redaction must remain. Existing worktrees are external Git state: discover and report; never silently adopt/remove.

## Profiles and deletion

Profiles isolate config, credentials, sessions, telemetry, hooks, cron, plugins and user memory. Uninstall preserves credentials unless `--purge` and does not remove sessions/config generally (`uninstall.py`; tests `test_uninstall.py`). Rust uninstall/migration must follow the same preservation principle and target only files it owns.

## Migration approach

1. Implement read-only inventory/import scanner with no writes.
2. Golden-decode real sanitized schema-1 sessions and nested states.
3. Add dual-reader: Vesper current schema first, Python legacy fallback.
4. Initially write either a new Vesper location or byte-compatible schema 1; never dual-write without transaction/recovery design.
5. Make derived indexes rebuildable and record source revision.
6. Add explicit `vesper migrate --dry-run`, backup manifest, checksums, and rollback.
7. Move individual stores only after reader/writer/downgrade tests.
8. Retain legacy files until explicit user-approved cleanup after a full release cycle.

## Corruption and concurrency policy for Rust

- Parse into bounded DTOs with unknown-field preservation where round-trip matters.
- Quarantine corrupt new-store records; never overwrite the corrupt original automatically.
- Use same-directory temp, flush/fsync file, atomic rename, optionally fsync parent.
- Use advisory locks plus revision tokens for cross-process stores.
- SQLite uses transactions, WAL, busy timeout and migrations; index remains disposable.
- JSONL appends remain one encoded line under size bound and lock where multi-process writes occur.
- Every destructive migration has a manifest of before/after hashes.

## Explicit compatibility gates

- Load/replay/fork a Python session with identical visible history/settings/lineage.
- Search legacy sessions with equivalent user/assistant results and no newly exposed secrets.
- Read/write all project memory/skill/bundle files without loss.
- List/run/renew/finish a version-1 cron job exactly once across Python/Rust contenders.
- Verify/install existing signed plugins.
- List/create/rollback/migrate Python checkpoints.
- Preserve credentials/uninstall boundaries.
- Reject corrupt/unsupported versions without destructive repair.

## Unresolved uncertainties

- There is no explicit migration for future session versions beyond permissive field defaults.
- `mcp.json` write atomicity/permissions are weaker than other stores.
- Session concurrent-writer behavior is last-write-wins.
- Windows mode/ACL parity cannot be inferred from POSIX chmod code and needs native tests.

These are design risks, not blockers to beginning fixtures and architecture foundation.
