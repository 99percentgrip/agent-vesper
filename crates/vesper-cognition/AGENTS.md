# vesper-cognition — mem0-equivalent cognitive memory engine

## Purpose

Own the provider-neutral **cognitive memory engine** (ADR 0015 — Stage 16): a
native Rust emulation of the mem0 V3 ("April 2026 new algorithm") oracle at
`/home/alex/Projects/mem0` (pin `29fa4155`). Single-pass ADD-only extraction,
hybrid semantic + FTS5 BM25 + entity-boost retrieval, and an embedded SQLite
backing. This is the subsystem that backs the TUI's cognitive-memory surface
(`/remember`, `/recall`, `/forget` once the command layer is wired).

This crate is **independent of** `vesper-memory`: `vesper-memory` continues to
own the append-only JSONL memory graph, learned skills, user profile, and
bounded awareness ledger (ADR 0011). The two subsystems do not share storage
or types.

## Ownership

- `src/lib.rs` — public re-exports, `CognitiveMemory` facade, `CognitiveConfig`,
  and the `open()` entry point.
- `src/error.rs` — `CognitionError` (sanitized; never leaks paths, payloads,
  or secrets).
- `src/types.rs` — public value types: `Message`, `Scope`, `Attribution`,
  `MemoryRecord`, `MemoryHit`, `ScoreBreakdown`, `MemoryEvent`, `HistoryEvent`.
- `src/ports.rs` — trait ports: `EmbeddingPort`, `ExtractionLlmPort`,
  `EntityExtractorPort`, plus `EmbedAction` and the `CognitionPorts` bundle.
  **No concrete impls in this crate.**
- `src/prompts.rs` — V3 prompt ports: the system prompt, agent-scoped suffix,
  procedural prompt, and the user-side prompt builder
  (`generate_additive_extraction_prompt`).
- `src/extract.rs` — extraction-response parsing with the
  `remove_code_blocks` → JSON → `extract_json` brace-fallback chain.
- `src/nlp.rs` — Snowball (`rust-stemmers`) lemmatization fallback and the
  regex entity extractor (PROPER / QUOTED / TOPIC / IDENTIFIER).
- `src/bm25.rs` — BM25 sigmoid normalization and query-length-adaptive
  parameters (verbatim port of `mem0/utils/scoring.py`).
- `src/score.rs` — hybrid scoring: `score_and_rank`, cosine similarity, and
  the entity-boost formula.
- `src/filters.rs` — metadata filter DSL (eq/ne/in/nin/gt/gte/lt/lte/contains/
  icontains/wildcard/AND/OR/NOT).
- `src/store.rs` — `CognitiveStore`: SQLite schema bootstrap, FTS5 BM25 index,
  relational CRUD primitives, and entity-graph operations.
- `src/pipeline.rs` — the 8-phase `add()` pipeline, hybrid `search()`, and the
  admin `update`/`delete`/`history`/`add_procedural` operations.
- `assets/*.txt` — verbatim ports of the oracle's
  `ADDITIVE_EXTRACTION_PROMPT`, `AGENT_CONTEXT_SUFFIX`, and
  `PROCEDURAL_MEMORY_SYSTEM_PROMPT`. Bundled via `include_str!`.

## Local Contracts

- Depends only on `vesper-domain`, `vesper-security`, `rusqlite` (bundled),
  `rust-stemmers`, and the standard utility crates (`serde`, `serde_json`,
  `thiserror`, `chrono`, `uuid`, `md-5`, `regex`). No provider, runtime, ACP,
  sessions, agent, testkit, HTTP, or TUI dependency.
- This is the **only** production crate permitted to declare `rusqlite`. The
  historical blanket Stage-5 SQLite prohibition is superseded by a per-crate
  allowlist exception in `cargo xtask architecture`.
- `#![forbid(unsafe_code)]` is enforced at the crate root. The bundled SQLite
  amalgamation lives in `libsqlite3-sys`, not in this crate's source.
- All filesystem writes are confined to the absolute path passed at
  construction. Refuses a non-absolute path or a path whose parent does not
  exist (same confinement rule as the Stage 6 writer and `vesper-memory`).
  The crate never creates the parent directory.
- Provider-bound secrets never enter the crate. They are resolved at the
  composition boundary and used by the trait impls in the binary.
- `Send + Sync` via interior mutex on the SQLite connection. The composition
  boundary can share one `Arc<CognitiveMemory>` across the TUI event loop.
- `CognitionError` messages are secret-safe: never include file contents, API
  keys, full paths, or extracted memory text.

## Work Guidance

- When porting a new mem0 behavior, cite the oracle `file:line` in the doc
  comment and update the foundation blueprint if the parity surface changes.
- The trait ports are deliberately narrow. Do not add provider-specific
  arguments; route provider concerns through the composition boundary.
- FTS5 keyword-search scope filtering uses the `scope_match` SQL function
  registered by `CognitiveStore::open_with_functions`. The simpler `open`
  constructor is reserved for tests that do not exercise keyword search.
- Entity-linking failures are non-fatal (mirrors the oracle's swallow-at-
  warning rule); they never break the primary ADD path.

## Verification

- `cargo test -p vesper-cognition` — unit + integration tests covering the
  extraction pipeline (12 oracle prompt examples), MD5 dedup regression,
  BM25 sigmoid parametrization (5 rows), hybrid scoring divisor adapts to
  active signals (1.0 / 1.5 / 2.0 / 2.5), entity-boost bounds (≤0.5;
  memory-count-weight curve), Filter DSL operators, rolling 10-message
  window eviction, procedural-memory compaction, cosine similarity, FTS5
  BM25 round-trip.
- `cargo xtask architecture` — confirms the new crate satisfies the
  production dependency allowlist (with the per-crate `rusqlite` exception),
  the source-tree unsafe ban, and the source-scanner forbidden-reference
  rules.
- `cargo xtask verify` — full workspace fmt + clippy `-D warnings` + tests +
  doc tests + architecture + provider/runtime/acp/sessions checks.

## Child DOX Index

No children.
