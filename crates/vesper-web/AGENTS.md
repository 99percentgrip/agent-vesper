# vesper-web

## Purpose

Own the VRO-14 PR-1 Perception Engine core: the pure-logic
Parse → Strip → Prune → Convert pipeline that maps fetched HTML bytes to
bounded Markdown (`docs/web-oracle-extraction-prd.md`, Feature 1).

## Ownership

- `src/dom.rs` — lenient HTML parser over `quick-xml` (tree `Document`),
  entity decoding, void-element handling, and sibling implicit-close rules.
  `find_first` is the production element lookup (title/meta/canonical walks).
- `src/arena.rs` — arena projection with precomputed density metrics for
  the prune stage; `pre_order`/`post_order`/`parent_of` traversal surface.
- `src/strip.rs` — alpha-intent unwanted-element removal (script/style/
  noscript/svg/nav/footer/header/aside/form + hidden elements).
- `src/density.rs` — beta's `PruningContentFilter` port: exact default
  weights (0.4/0.2/0.2/0.1/0.1), fixed 0.48 / dynamic thresholds,
  `min_word_threshold` sentinel, preserve classes/tags with whole-subtree
  preservation.
- `src/convert.rs` — baseline Markdown converter (ATX headings, GFM
  tables, fenced code, nested lists, hard breaks, inline emphasis).
- `src/pipeline.rs` — stage composition + per-stage byte accounting
  (`DensityReport`).
- `src/bm25.rs` — beta's `BM25ContentFilter` port: pure Rust Okapi BM25
  (k1 1.5, b 0.75) with English stemming (`rust-stemmers`), beta's
  priority-tag boost, default threshold 1.0, and `min_word_threshold`
  word floor. `score_all` exposes threshold-free ranking for tests and
  diagnostics; `filter` is the production gate.
- `src/snapshot.rs` — CDP `DOMSnapshot.captureSnapshot` serde types and
  materialization (flat + owned-tree forms, the pinned ten required
  computed styles, string-table resolution).
- `src/interactable.rs` — clickability heuristics (JS-listener flag,
  interactive ARIA roles/tags, form-control wrapper search) and the
  sensitive-value gate (password/file/hidden inputs, cc-*/one-time-code
  autocomplete → `•` masks, length-capped).
- `src/selector_map.rs` — the token-efficient numbered map and the
  `(session_id, backend_node_id)` index cache; retired indexes are never
  reassigned within a session (gamma's cross-step stability contract).
- `src/action.rs` — the `BrowserAction` enum (navigate/click/type/scroll/
  select/back/forward/reload/screenshot/close) and the model-facing
  action registry.
- `src/driver.rs` — the pipe-only CDP driver seam (`BrowserDriverPort`):
  NUL-framed JSON over the anonymous fd3/fd4 pair, no TCP anywhere,
  bounded command plans per action, fail-closed sandbox gating.
- `examples/gen-goldens.rs` — maintenance tool regenerating the golden
  corpus under `fixtures/web-oracle/goldens/`.

## Local Contracts

- Strictly zero I/O: no network, no filesystem, no clock. All input
  arrives as in-memory HTML strings; callers own fetching through the
  sandbox boundary (PRD Feature 4).
- `#![forbid(unsafe_code)]`; quick-xml is the only dependency.
- The naming rule (PRD §0) is absolute: upstream projects are referenced
  only as web oracle alpha/beta/gamma; `<pkg-root>` replaces any
  banned-token-bearing upstream path. The production-sources scan forbids
  referencing `spikes/` paths in this crate's source.
- Parser invariants (PR-0 lessons, unit-tested): `check_end_names=false`
  for HTML; void elements never take children; `tr`/`td`/`th`/`li`/`p`/
  `option`/`dt`/`dd` implicitly close open same-scope siblings;
  mismatched closers re-nest the unclosed chain instead of flattening it;
  `Event::GeneralRef` carries entities and must be decoded.
- Converter determinism is a test contract: identical input renders
  byte-identical output; any intentional change regenerates goldens via
  the example binary in the same commit.

## Work Guidance

- Fixtures live in `fixtures/web-oracle/` (see its AGENTS.md); the corpus
  integration test asserts every golden byte-for-byte on each run.
- When changing converter behavior deliberately, run
  `cargo run -p vesper-web --example gen-goldens -- <repo-root>` and
  commit the regenerated goldens together with the change.

## Verification

- `cargo test -p vesper-web`
- `cargo clippy -p vesper-web --all-targets -- -D warnings`
- `cargo run --package xtask --quiet -- architecture`

## Child DOX Index

No children.
