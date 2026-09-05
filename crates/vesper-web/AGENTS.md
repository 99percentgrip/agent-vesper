# vesper-web

## Purpose

Own the VRO-14 PR-1 Perception Engine core: the pure-logic
Parse → Strip → Prune → Convert pipeline that maps fetched HTML bytes to
bounded Markdown (`docs/web-oracle-extraction-prd.md`, Feature 1).

## Ownership

- `src/dom.rs` — lenient HTML parser over `quick-xml` (tree `Document`),
  entity decoding, void-element handling, and sibling implicit-close rules.
- `src/arena.rs` — arena projection with precomputed density metrics for
  the prune stage.
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
