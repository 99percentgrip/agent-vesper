# vesper-memory — persistent memory and bounded awareness

## Purpose

Own the provider-neutral **durable memory graph** (ADR 0011 — Stage 12):
project-local memory entries, learned skills, the cross-project user
profile, and the bounded in-process epistemic ledger. This is the
subsystem that backs the Tier C Phase 8 un-stubbed commands
(`/memory`, `/goal`, `/skills`, `/profile`, `/awareness`, `/deliberation`,
`/metacognition`, `/repository`, `/meta-learning`, `/observability`,
`/subgoal`, `/curator`, `/journey`).

## Ownership

- `src/lib.rs` — public re-exports and crate-level docs.
- `src/store.rs` — `MemoryStore` (append-only JSONL memory entries) with
  atomic write-to-temp + rename, mirroring the `vesper-sessions` writer.
- `src/skills.rs` — `SkillStore` (markdown skill files under
  `<root>/skills/<slug>.md`), `SkillSummary`, and bounded JSON skill bundles
  under `<root>/bundles/<slug>.json`.
- `src/skill_orchestrator.rs` — ADR 0024 deterministic skill metadata parser,
  eligibility gate, semantic/file/trigger ranker, bounded composer, transient
  loader, and secret-free in-process outcome tracker.
- `src/profile.rs` — `UserProfile` (single markdown file with bounded
  size, append/forget with category sections).
- `src/awareness.rs` — `AwarenessLedger` and the `EpistemicRecord` /
  `EvidenceEvent` / `RecordKind` value types (bounded, in-process,
  JSON-serializable).
- `src/error.rs` — `MemoryError` (sanitized; never leaks paths or
  payloads that may carry secrets).

## Local Contracts

- Depends only on `vesper-domain` and `vesper-security`. No provider,
  runtime, ACP, sessions, agent, testkit, SQLite, HTTP, or TUI dependency.
- All filesystem writes are atomic (write-to-temp + `fsync` + rename),
  confined to the absolute root passed at construction, and bounded by
  configured byte limits. Refuses a non-absolute root or a root whose
  parent does not exist (same confinement rule as the Stage 6 writer).
- Stores never create the root directory; the composition boundary
  (binary) is responsible for ensuring it exists.
- No live provider calls, no network I/O, no subprocess execution.
- All public types are `Send + Sync` and use interior locking; the
  composition boundary can share one `Arc<MemoryStore>` across the
  TUI event loop and (future) the agent loop.
- Records are bounded: summary ≤ 1024 chars, scope list ≤ 8 entries,
  evidence list ≤ 16 entries, total entries ≤ 10_000. Inputs that
  exceed bounds are rejected, not silently truncated.
- Skill bundles are validated before atomic replacement: at most 32
  validated skill slugs and 32 KiB serialized JSON per bundle.
- Learned-skill files are bounded: the store enumerates at most 500
  skill files and each body is at most 200 KB (raised 2026-08 so
  migrated curated reference skills round-trip through `learn_skill`).
- `SkillStore` has an optional cross-project global read layer
  (`open_with_global`): listings append global-only skills after local
  ones, local slugs shadow, reads fall back, and writes/bundles-merges
  follow the same precedence. The TUI roots it at
  `AGENT_VESPER_GLOBAL_MEMORY_ROOT` (default `~/.agent-vesper/memory`);
  a missing root silently disables the layer.
- `read_section` extracts one heading's section; the headline shown by
  `list_skills` prefers the frontmatter `description` (oracle context
  parity: `- {name}: {description}`).
- Skill orchestration considers metadata only, selects at most three skills,
  and fails closed on invocation policy, archive/platform/tool/exclusion/
  conflict constraints. Inline bodies are bounded to 24,000 characters each
  and 60,000 total; isolated bodies are never returned to the main context.
  Selection and bundle activation never grant tool or external-side-effect
  permission.
- Catalog discovery reads at most 32,000 bytes from each skill file. Full
  bodies are read only for selected inline skills; the main process never
  reads a selected isolated skill's full body.

## Work Guidance

- When adding a new memory kind, update `MemoryKind`, the resolver map
  in `apps/agent-vesper-tui/src/commands.rs`, and the dispatch handler.
- Keep `MemoryError` messages secret-safe: never include file contents,
  API keys, or full paths in error text. Use `vesper_security::path`
  helpers for path confinement.
- The append-only JSONL log is the source of truth for memory entries;
  `forget` rewrites the file by filtering lines (single atomic rename).
- The awareness ledger is the **in-process** epistemic state; persistence
  is opt-in via `save()`/`load()` to a single JSON file. The harness
  (not this crate) is responsible for keeping the live state coherent
  with provider evidence.
- Bundle files are durable grouping metadata only; bundles are activated
  explicitly, fail as a unit when a requested member is missing, ineligible,
  unreadable, or conflicting, rank members within the normal three-skill cap,
  and never implicitly execute, mutate, or widen permission.

## Verification

- `cargo test -p vesper-memory` — unit + integration tests (stores, routing
  against the curated catalog, eligibility, bundles, isolation, adversarial
  body non-selection, bounds, and feedback).
- `cargo xtask architecture` — confirms the new crate satisfies the
  production dependency allowlist and the source-tree unsafe ban.

## Child DOX Index

No children.
