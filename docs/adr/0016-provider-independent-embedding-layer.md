# ADR 0016 — Provider-Independent Embedding Layer

- **Status:** Accepted
- **Date:** 2026-08-11
- **Supersedes:** none (eliminates Gap 10 from the v0.20.11 cognition-migration audit)
- **Builds on:** ADR 0015 (Stage 16 `vesper-cognition`)

## Context

ADR 0015 introduced the cognitive memory engine with provider-routed embeddings —
the active chat provider (ZAI, LM Studio, future X) determined which embedder
was used to encode both stored memories and incoming queries. This produced a
cascade of failure modes documented in the post-v0.20.11 audit report:

- **Gap 10 (HIGH — silent recall death):** when the embedder's dimension did
  not match the stored memory vectors, `cosine()` silently returned 0.0 for
  every memory. The user experienced "the AI forgot everything" with no log
  signal.
- **Gap 11 (HIGH — usability killer):** rapid provider switching (ZAI 1024-d
  ↔ LM Studio 768-d) forced a full re-embed of every stored memory on every
  switch.
- **Gap 12 (MED):** an offline embedding endpoint at search time killed
  auto-recall for the entire session.

v0.20.12 and v0.20.13 mitigated these gaps (one-time drift log, model-aware
migration, batched re-embed, cold-start status). But the fundamental issue
remained: **the embedder was chained to the chat provider**, so any provider
switch could break cosine similarity.

## Decision

Decouple the embedding source from the active chat provider entirely. The
embedding model is determined by an explicit configuration file
(`.agent-vesper/cognition/embedding.json`) — NOT by which provider is active
for chat. Switching chat providers no longer changes the embedder.

### Schema

```json
{
  "source": "lmstudio" | "bigmodel" | "local",
  "endpoint": "http://localhost:1234/v1/embeddings",
  "model": "text-embedding-nomic-embed-text-v1.5",
  "api_key": null,
  "dimension": 768
}
```

When `source` is absent (or the file is missing), the bundle falls back to
the v0.20.13 provider-routed behavior. **This preserves backward compatibility
with zero migration cost for existing user installs.**

### SearchMode (live atomic state)

`CognitiveMemory` exposes a `SearchMode` enum (`Hybrid` | `BM25Only`) backed
by an `AtomicU8`. The composition boundary sets the mode after a startup
probe:

- Embedder reachable + dimension matches the store → `Hybrid` (full
  semantic + BM25 + entity-boost pipeline)
- Embedder unreachable OR produces incompatible vectors → `BM25Only`
  (keyword-only recall, never returns `Err`)

`search()` automatically downgrades from `Hybrid` to `BM25Only` on the first
embedder failure mid-search, logs ONCE per process, and continues with
keyword-only recall for that turn. The next successful embedder call
auto-upgrades back to `Hybrid`. **This is the elimination of Gap 10 — silent
recall death is now structurally impossible.**

### Why BM25-only instead of an empty result?

The previous behavior was to return `Err` (or `Ok([])` if wrapped with
`.ok()?`) when the embedder failed. Both presented to the user as "the AI
forgot everything". With ADR 0016, keyword recall via FTS5 BM25 still works
without any embedding call — recall is degraded but not dead. Genuine
keyword matches surface even when the embedding endpoint is completely down.

## Consequences

### Positive

- **Gap 10 ELIMINATED**: dimension mismatch is no longer possible at runtime
  for users with an `embedding.json`. Cosine similarity cannot silently fail.
- **Gap 11 ELIMINATED**: rapid provider switching no longer triggers
  re-embedding. The embedder is independent.
- **Gap 12 ELIMINATED**: an unreachable endpoint degrades to BM25-only
  recall, not dead recall.
- **Multi-provider harness becomes true**: switching from LM Studio to ZAI
  to a future provider X leaves cognitive memory fully functional. No
  restart, no migration, no data loss.

### Negative

- One additional config file for users who want provider-independent
  embeddings. Backward-compat fallback means existing users don't need to do
  anything until they want this capability.
- `search_bm25_only()` is a separate code path with its own threshold
  scaling (0.4× the hybrid threshold). The scaling is documented inline;
  future tuning may need a config knob.

### Neutral

- The `EmbeddingPort` trait gains a `model_name()` method (added in v0.20.13).
- The `cognition_meta` table (added in v0.20.13) continues to store the
  active embedding model for migration detection — but under ADR 0016,
  migrations are genuinely rare because the embedder rarely changes.

## Verification

- `cargo test -p vesper-cognition` — 56 tests including:
  - `bm25_only_fallback_returns_keyword_hits_when_embedder_fails` (the Gap 10
    elimination proof)
  - `explicit_bm25_only_mode_skips_embedder_entirely`
  - `search_mode_defaults_to_hybrid`
  - `set_search_mode_round_trips`
- `cargo xtask verify` (canonical CI gate)
- `cargo xtask architecture` (22 packages)

## Future Work

- All three v0.20.14 deferrals are closed in v0.20.15:
  - **Directive 1 — BigModel source path**: `build_independent_embedder`
    takes a `credential_source` argument; `source: "bigmodel"` now constructs
    `BigModelEmbeddingAdapter` (resolves JWT per call from the ZAI
    credential) instead of falling back to `LocalHashEmbedder`. The test
    `embedding_bigmodel_source_path_constructs_bigmodel_adapter` proves the
    embedder is NOT `local-hash-embedder`.
  - **Directive 2 — Background-thread embedder probe**: `open_default` no
    longer performs an eager blocking HTTP probe. The engine starts in
    `BM25Only` (or `Hybrid` for `source: "local"` / ZAI-routed BigModel),
    and a `std::thread::spawn` background task issues a one-shot `embed()`
    call; on success it flips the engine to `Hybrid`. TUI startup is now
    instant regardless of LM Studio availability.
  - **Directive 3 — `/embedding` slash command**: registered in the
    Vesper-native surface (alongside `/auth`, `/provider`, `/lmstudio`).
    `/embedding` shows live status; `/embedding set source=... endpoint=...
    model=... api_key=... dimension=...` parses, validates, persists to
    `embedding.json`, and triggers a hot-reload that probes the new endpoint
    in a background thread; `/embedding clear` deletes the file. The parser
    is pure and unit-tested (`EmbeddingPairs::parse`).
- The `/embedding set` hot-reload probes the new endpoint in a background
  thread — same pattern as the startup probe.

## References

- Gap report `fixthegaps.md` (this session)
- ADR 0015 — Stage 16 `vesper-cognition`
- v0.20.12 release — closed gaps 1, 2, 6, 7, 9, 10 (mitigation), 12 (mitigation), 13
- v0.20.13 release — closed gaps 3, 4, 5, 8, 11
- v0.20.14 release — ADR 0016 implementation
- v0.20.15 release — Directive 1 (BigModel), Directive 2 (background probe),
  Directive 3 (`/embedding` slash command)
