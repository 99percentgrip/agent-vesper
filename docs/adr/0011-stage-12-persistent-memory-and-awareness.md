# ADR 0011: Stage 12 — Persistent Memory, Learned Skills, User Profile, and Bounded Awareness

Status: ACCEPTED

Builds on: [ADR 0010](0010-tier-c-agent-loop-and-tool-execution.md) Phase 7
(the `Deferred { command, reason }` registry variant).

## Context

ADR 0010 Phase 7 shipped a 100%-complete *registry* of the oracle's
`LOCAL_COMMANDS` surface, but 13 of those commands — the awareness/memory
family — resolved to `Deferred { command, reason }` notices naming a
"persistent-goal awareness subsystem (Stage 16+, deferred)". The lead
architect rejected that deferral as a stub: every command in the surface
must be backed by a real, persistent subsystem.

### Oracle data model (audit)

The frozen Python oracle at `bf4d4287` ships the durable memory surface in
two production modules:

- `glm_acp/memory.py` (1222 LOC): project-local memory at
  `.glm-acp/memory.md` (append-only markdown), learned skills under
  `.glm-acp/skills/<slug>/SKILL.md`, and the cross-project user profile at
  `~/.config/glm-acp/user.md`. Atomic writes via tempfile + rename;
  bounded reads (`MAX_MEMORY_CHARS = 32_000`).
- `glm_acp/awareness.py` (544 LOC): the bounded `EpistemicLedger`
  (`MAX_RECORDS = 100`, `MAX_EVIDENCE_EVENTS = 200`) of
  `EpistemicRecord { kind, summary, scopes, evidence, supports,
  confidence, status }`. In-process; JSON-serializable.

### Commands this ADR un-stubs

Thirteen commands move from `Deferred` to a real, persistent backing:

| Command | Op | Backing |
|---|---|---|
| `/memory [needle]` | `MemoryList` | `MemoryStore` |
| `/goal <text>` | `GoalAdd` | `MemoryStore` |
| `/subgoal <text>` | `SubgoalAdd` | `MemoryStore` |
| `/skills` | `SkillsList` | `SkillStore` |
| `/profile` | `ProfileShow` | `UserProfile` |
| `/awareness [kind]` | `AwarenessList` | `AwarenessLedger` |
| `/metacognition` | `MetacognitionList` | `AwarenessLedger` |
| `/deliberation` | `DeliberationList` | `AwarenessLedger` |
| `/repository` | `RepositoryList` | `AwarenessLedger` |
| `/meta-learning` | `MetaLearningList` | `AwarenessLedger` |
| `/observability` | `ObservabilityList` | `AwarenessLedger` |
| `/curator` | `Curate` | `MemoryStore::curate` |
| `/journey` | `Journey` | composite `MemoryStore` + `SkillStore` |

## Decision

1. **New crate `vesper-memory`** owns the durable memory graph, learned
   skills, user profile, and bounded epistemic ledger. It depends only on
   `vesper-domain` and `vesper-security` (no provider, runtime, ACP,
   sessions, agent, testkit, SQLite, HTTP, or TUI dependency).

2. **Storage layout** under one configurable root directory:
   - `memory.jsonl` — append-only `MemoryEntry` log (the source of truth
     for `/memory`, `/goal`, `/subgoal`).
   - `skills/<slug>.md` — one markdown file per learned skill.
   - `user.md` — cross-project user profile.
   - `awareness.json` — persisted epistemic ledger.

   All writes are atomic (write-to-temp + `fsync` + rename), confined to
   the absolute root, and bounded by configured byte limits — the same
   discipline as the Stage 6 session writer.

3. **Confinement rule** mirrors `vesper-sessions`: the root must be
   absolute and its parent must exist. The crate never creates the root;
   the composition boundary owns the path.

4. **Bounded contract**: summary ≤ 1024 chars, scopes ≤ 8, evidence ≤ 16,
   ids ≤ 64 chars, memory entries ≤ 10_000, awareness records ≤ 100,
   skill files ≤ 500, skill body ≤ 200 KB (raised 2026-08 to admit migrated curated skill libraries), profile ≤ 16 KB. Skills additionally have an optional cross-project global read layer: project-local skills shadow global ones, reads fall back, writes stay project-local (amendment 2026-08, migration of the curated library to `~/.agent-vesper/memory/skills/`). Inputs that
   exceed bounds are rejected, not silently truncated.

5. **TUI wiring** follows the existing `pending_*` drain pattern:
   - The resolver returns `CommandOutcome::Memory(MemoryOp)`.
   - `dispatch` records `SessionState.pending_memory_op: Option<MemoryOp>`.
   - The binary owns a `MemoryStores` bundle and drains the op after
     dispatch (mirroring `pending_prompt` / `pending_reasoning`).

6. **Composition boundary root**: `AGENT_VESPER_MEMORY_ROOT` env var, with
   a fallback to `.agent-vesper/memory/` under the current directory. If
   any store cannot be opened, that store stays `None` and memory commands
   surface a clear "store unavailable" notice rather than crashing the TUI.

7. **The `Deferred` variant stays** for the remaining ~41 commands that
   depend on Stage 14 (worktrees/checkpoints) or Stage 15 (MCP/plugins) —
   those subsystems ship in subsequent ADRs. The 13 awareness/memory
   commands are no longer in the deferred list; they resolve to real
   `Memory(MemoryOp)` outcomes.

## Consequences

- **Positive**: 13 commands move from stubbed to production-functional.
  The agent can now persist goals, query memories, list learned skills,
  and surface epistemic state across sessions — the durability the
  architect demanded.
- **Positive**: the new crate is small (one bounded contract, four stores,
  atomic writes mirroring the existing Stage 6 pattern), so it is cheap
  to review and maintain.
- **Positive**: the wiring is additive — `Memory(MemoryOp)` is a new
  `CommandOutcome` arm; the existing `Deferred` arm is untouched for the
  remaining 41 commands.
- **Negative**: storage uses JSONL/markdown files, not a relational store.
  SQLite is banned by `deny.toml` for Stage 5 reasons; the file-based
  store is correct for the bounded contract (≤ 10_000 entries) but would
  need re-evaluation if the entry cap grew substantially.
- **Trade-off**: the awareness ledger is in-process; persistence is opt-in
  via `AwarenessLedger::save()` / `load()`. The harness (not this crate)
  is responsible for keeping the live state coherent with provider
  evidence — this matches the oracle's separation.

## Verification

- `cargo test -p vesper-memory` — 27 unit/integration tests covering
  confinement, atomic writes, bounds enforcement, append/forget/curate
  idempotency, profile sectioning, and awareness upsert/resolve/save/load.
- `cargo test -p agent-vesper-tui --lib` — 4 new Phase 8 tests:
  `phase8_memory_command_stashes_a_pending_memory_op`,
  `phase8_goal_command_stashes_goaladd_op`,
  `phase8_goal_without_argument_errors_instead_of_deferring`,
  `phase8_all_thirteen_memory_commands_record_pending_ops`. Plus 3
  commands.rs tests proving every one of the 13 commands resolves to a
  real `Memory(MemoryOp)` outcome.
- `cargo xtask architecture` — 16 packages validated (was 15).
- `cargo xtask verify` — 477 tests pass, 0 failures (was 443).
- `phase7_every_deferred_command_names_a_subsystem` updated to assert the
  remaining 41 deferred commands still resolve to `Deferred` — the 13
  awareness/memory commands are no longer in that list.

## Migration matrix (Stage 12 progress)

| Category | Shipped in this ADR | Still deferred |
|---|---|---|
| Memory subsystem (13) | memory, goal, subgoal, skills, profile, awareness, metacognition, deliberation, repository, meta-learning, observability, curator, journey | — |
| Worktree & checkpoints (9) | — | sessions-new, sessions, lineage, branch, rename, checkpoint, rollback, rewind, undo |
| MCP & plugins (2) | — | mcp, plugins |
| Cron/loop (1) | — | loop |
| CI integration (1) | — | ci |
| Export/clipboard (2) | — | export, copy |
| Composer (6) | — | history, search, prompt, btw, blocks, annotate |
| ratatui UI rebuild (8) | — | theme, vim, keybinds, statusline, screen-reader, native-mouse, reasoning-panel, toggle-thinking |
| Live session settings (6) | — | settings, permission, mode, generation, auxiliary, mixture |
| Image subsystem (4) | — | image, attach, image-render, screenshot |
| Mobile/sound (2) | — | mobile, sound |
| **TOTAL** | **13 un-stubbed** | **41 still deferred (Stages 14/15+)** |
