# web-oracle-portability (VRO-14 PR-0 spike)

Disposable portability spike for VRO-14 PR-0 (`docs/web-oracle-extraction-prd.md`).
Two objectives:

1. **Converter & density portability** — a quick-xml DOM, a minimal
   markdown converter, and a port of beta's `PruningContentFilter`
   (TextDensityFilter), measured over 12 offline fixtures.
2. **Headless CDP pipe feasibility** — spawn a Chrome-family headless
   binary with `--remote-debugging-pipe`, exchange CDP JSON over fd3/fd4
   anonymous pipes (no TCP), and prove it at the kernel level.

Standalone Cargo workspace (`spikes/AGENTS.md` contract). Fully offline:
all dependencies are vendored under `vendor/` and pinned in `Cargo.lock`.

## Names

Only `alpha` / `beta` / `gamma` (+ `<pkg-root>` for gamma's package
directory) are used — never brand names (PRD §0 naming rule).

## Layout

- `src/dom.rs` — quick-xml HTML5-lenient parser → `Document` (tree) and
  `Dom` (arena) surfaces; `strip` (alpha's removeUnwantedElements intent +
  beta's excluded_tags).
- `src/density.rs` — beta's PruningContentFilter port (weights 0.4/0.2/
  0.2/0.1/0.1, fixed 0.48 threshold, dynamic modifiers, min_word_threshold
  −1.0 sentinel, preserve classes/tags) + the measurement driver
  (`cargo run -- density fixtures`).
- `src/convert.rs` — minimal markdown converter (ATX headings, links,
  GFM tables, fenced code, single_line_break option, images stripped by
  default) with alpha-style post-processing.
- `src/cdp.rs` — `PipeCdp`: spawn via `/bin/sh -c 'exec "$0" … 3<&0 4>&1'`
  (safe fd mapping, no unsafe), NUL-delimited CDP JSON framing, flat
  session calls, kernel-level no-TCP proof (`cargo run -- cdp /usr/bin/google-chrome`).
- `fixtures/f01..f12-*.html` — 12 deterministic, brand-free offline pages
  (article, docs, landing, SPA shell, forum, table-heavy, API docs, wiki,
  nav-heavy settings, listicle, edge cases, e-commerce).
- `VERDICT.md` — findings and go/no-go recommendations.

## Verification

```
cd spikes/web-oracle-portability
cargo test --locked --offline
cargo run --locked --offline -- density fixtures
cargo run --locked --offline -- cdp /usr/bin/google-chrome
```

Requires a Chrome-family binary for objective 2 (the machine has
`google-chrome`; a dedicated headless-shell build is a PR-1 packaging
decision recorded in VERDICT.md).

## Disposal

Throwaway by contract: nothing here is production code. Port the verified
behaviors (not the files) into `vesper-web` in PR-1.
