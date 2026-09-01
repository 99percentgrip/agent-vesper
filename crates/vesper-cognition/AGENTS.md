# vesper-cognition — memory-oracle-equivalent cognitive memory engine

## Purpose

Own the provider-neutral **cognitive memory engine** (ADR 0015 — Stage 16): a
native Rust emulation of the memory oracle V3 ("April 2026 new algorithm") at
`/home/Alex/Projects/memory-oracle` (pin `29fa4155`). Single-pass ADD-only extraction,
hybrid semantic + FTS5 BM25 + entity-boost retrieval, and an embedded SQLite
backing. This is the subsystem that backs the TUI's cognitive-memory surface
(`/remember`, `/recall`, `/forget`, `/memories`, `/promote`, and `/demote`).
The crate owns one store per `CognitiveMemory` instance; ADR 0021's independent
global and project instances and routing policy belong to the TUI composition
boundary, not this provider-neutral engine.

This crate is **independent of** `vesper-memory`: `vesper-memory` continues to
own the append-only JSONL memory graph, learned skills, user profile, and
bounded awareness ledger (ADR 0011). The two subsystems do not share storage
or types.

## Ownership

- `src/lib.rs` — public re-exports, `CognitiveMemory` facade, `CognitiveConfig`,
  the shared model-facing capability instruction, and the `open()` entry point.
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
  parameters (verbatim port of the oracle `utils/scoring.py`).
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

- When porting a new memory-oracle behavior, cite the oracle `file:line` in the doc
  comment and update the foundation blueprint if the parity surface changes.
- The trait ports are deliberately narrow. Do not add provider-specific
  arguments; route provider concerns through the composition boundary.
- FTS5 keyword-search scope filtering uses the `scope_match` SQL function
  registered by `CognitiveStore::open_with_functions`. The simpler `open`
  constructor is reserved for tests that do not exercise keyword search.
- Entity-linking failures are non-fatal (mirrors the oracle's swallow-at-
  warning rule); they never break the primary ADD path.
- **Embedder-swap migration surface** (`v0.20.12`+): when the active
  embedder's dimension changes (e.g. `LocalHashEmbedder` 1024-d → neural
  768-d), the composition boundary calls `reembed_everything()` which
  delegates to `reembed_all()` (memories) + `reembed_all_entities()`
  (entities). Both pull from `all_memory_reembed_targets` /
  `all_entity_reembed_targets` — full-table scans with NO 10k LIMIT and NO
  expiry filter, closing the silent-truncation (Gap 2) and silent-skip
  (Gap 1) bugs. Entities have their own embedding column used by
  `entity_boosts`; leaving them on the old dimension silently zeroes every
  boost after a swap (Gap 7 — fixed). On per-row failure the migration
  aborts with a partial-state log (Gap 6 — fixed).
- **Live embedder replacement**: `CognitiveMemory::replace_embedder` atomically
  replaces the adapter and expected vector dimension for subsequent calls.
  Both production hosts invoke it before `reembed_everything`; setting an
  embedding configuration must never merely probe an unused adapter.
- **Batched embedding in migration** (`v0.20.13`, Gap 3): `reembed_all`
  and `reembed_all_entities` use `embed_batch` in chunks of 256 instead of
  per-item `embed()`. N memories → ceil(N/256) HTTP round-trips. The
  `EmbeddingPort` trait exposes `embed_batch` (default falls back to
  per-item); concrete adapters override for native batch APIs
  (`LmStudioEmbedder` posts the full `input` array in one request).
- **Model-aware migration detection** (`v0.20.13`, Gap 11): the
  `cognition_meta` table stores `embedding_model` + `embedding_dim`. The
  composition boundary compares the active embedder's `model_name()`
  against the stored value, eliminating false-positive migrations between
  two models that happen to share a dimension, and false-negative
  migrations when the first stored row happened to match. `EmbeddingPort`
  exposes `model_name() -> &str` (default `"unknown"`); `LocalHashEmbedder`
  overrides with `"local-hash-embedder"`; `LmStudioEmbedder` returns its
  configured model. `CognitiveMemory::get_meta` / `set_meta` /
  `embedder_model_name` expose the surface to the binary.
- **Batched recall-count UPDATE** (`v0.20.13`, Gap 4):
  `increment_recall_counts` issues a single `UPDATE memories SET
  recall_count = recall_count + 1 WHERE id IN (?, ?, ...)` instead of N
  individual UPDATEs (top_k=5 → 5 statements → 1).
- **`search()` scope contract**: search callers MUST set `user_id`. A
  `debug_assert!` enforces it because `scope_match_field(None, _)` returns
  `true` for any stored value — a latent cross-user leak (Gap 13).
  `reembed_all` legitimately uses `Scope { all None }` but goes through
  `all_memory_reembed_targets`, never through `search`.
- **Dimension-drift diagnostic** (`v0.20.12`+): when `query_embedding.len()
  != stored_embedding.len()`, `cosine()` returns 0.0 silently — `search()`
  now logs ONCE per process via a `static AtomicBool` so the user sees the
  drift instead of experiencing "the AI forgot everything".
- **Provider-independent embedding layer** (`v0.20.14`, ADR 0016; refined
  `v0.20.15`): the embedding source is decoupled from the active chat
  provider via `.agent-vesper/cognition/embedding.json`. Switching chat
  providers (ZAI ↔ LM Studio ↔ future X) no longer changes the embedder —
  cosine cannot silently break. When the file is absent, the v0.20.13
  provider-routed behavior is the backward-compat fallback. Live
  `SearchMode` (`Hybrid` | `BM25Only`) is exposed via
  `CognitiveMemory::search_mode()` / `set_search_mode()`. `search()`
  auto-downgrades to `BM25Only` on the first embedder failure mid-search
  and auto-upgrades back to `Hybrid` on the next successful call — **Gap 10
  (silent recall death) is structurally eliminated**. `BM25Only` skips
  every embedding call and uses FTS5 keyword recall only; never returns
  `Err` for embedder reasons.
  - **v0.20.15 follow-ups**: (1) `source: "bigmodel"` constructs
    `BigModelEmbeddingAdapter` directly with a `credential_source`
    argument (resolves JWT per call from the ZAI credential) instead of
    falling back to `LocalHashEmbedder` — verified by
    `embedding_bigmodel_source_path_constructs_bigmodel_adapter`. (2) The
    startup probe runs in a `std::thread::spawn` background task; the
    engine starts in `BM25Only` and upgrades to `Hybrid` when the probe
    succeeds — TUI startup is now instant. (3) The `/embedding` slash
    command (Status/Set/Clear) is registered in the Vesper-native surface
    and drains through `pending_embedding_op`; Set hot-reloads the
    embedder with another background probe.

## Verification

- `cargo test -p vesper-cognition` — unit + integration tests covering the
  extraction pipeline (12 oracle prompt examples), MD5 dedup regression,
  BM25 sigmoid parametrization (5 rows), hybrid scoring divisor adapts to
  active signals (1.0 / 1.5 / 2.0 / 2.5), entity-boost bounds (≤0.5;
  memory-count-weight curve), Filter DSL operators, rolling 10-message
  window eviction, procedural-memory compaction, cosine similarity, FTS5
  BM25 round-trip, embedder-swap migration (Gaps 1/2/7/13 regression
  suite).
- `cargo xtask architecture` — confirms the new crate satisfies the
  production dependency allowlist (with the per-crate `rusqlite` exception),
  the source-tree unsafe ban, and the source-scanner forbidden-reference
  rules.
- `cargo xtask verify` — full workspace fmt + clippy `-D warnings` + tests +
  doc tests + architecture + provider/runtime/acp/sessions checks.

## Child DOX Index

No children.
