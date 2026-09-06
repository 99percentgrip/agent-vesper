# VRO-14 Final Audit — The Web Oracle Extraction

Status: COMPLETE. PR-0 through PR-6 landed (`docs/web-oracle-extraction-prd.md`).
Audit date: 2026-09-06.

## 1. Test floor

| PR | Floor after merge | Delta |
| --- | --- | --- |
| PRD baseline | 1,420 | — |
| PR-0 (portability spike) | 1,458 | +38 (spike-external; workspace floor unchanged by the excluded crate, +38 from PRD-era regression fills) |
| PR-1 (perception core) | 1,495 | +37 |
| PR-2 (extraction layer) | 1,523 | +28 |
| PR-3 (sandboxed fetch) | 1,568 | +45 |
| PR-4 (action engine) | 1,612 | +44 |
| PR-5 (tool wiring + parity) | 1,635 | +23 |
| PR-6 (adversarial + perf) | 1,644 | +9 |

Final: `cargo test --workspace --all-features` → 87 suites, **1,644 passed, 0 failed**,
1 ignored (the live-docker fetch proof, by design). Monotonic at every merge;
no test was deleted or weakened to admit a feature.

## 2. Performance gates (release profile, `--ignored` tests)

| Gate | Bound | Measured | Margin |
| --- | --- | --- | --- |
| Prune + Convert, 100k-node DOM | 150 ms | **36.4 ms** | 4.1× |
| Serialize 5k-interactable CDP snapshot | 50 ms | **1.2 ms** | ~40× |

Both live as `#[ignore]`-gated tests
(`crates/vesper-web/tests/adversarial.rs::perf`) and were validated locally
with `cargo test --release -p vesper-web --test adversarial -- --ignored`.

**The 100k gate failed at first measurement (829 ms)** and was fixed, not
loosened: `Dom::from_document` computed `el.text()`/`el.content_len()` per
node — each an O(subtree) walk — making deep chains O(N²). Metrics are now
accumulated bottom-up in one post-order pass (O(N) total); the projection
plus the whole prune+convert pipeline now sits at 36 ms with 4× headroom.

The zero-cost opt-in proof is structural (not a benchmark):
`crates/vesper-harness/src/web_service.rs::zero_cost_boot` proves the
absent-`[web]` boot constructs no `WebService`, registers no web tool under
any mode, and keeps the advertisement identical to the pre-web registry —
with a counter-test proving the disabled assertions bite (an attached scope
provably changes the registry).

## 3. Adversarial resilience

Corpus: `fixtures/web-oracle/adversarial/` (offline, deterministic):

| Fixture | Shape | Proven behavior |
| --- | --- | --- |
| a01-100k-nodes | 2.1 MB, ~52k elements in a 5,200-deep chain | All stages complete; content reachable; fit ≤ full. **Found a real stack overflow** — fixed with `MAX_PARSE_DEPTH` (browser-style flattening past the cap, mirroring Chrome's ~512-deep parser limit). |
| a02-malformed | unquoted attrs, unclosed tags, mixed case, broken entities, broken table | No panic; recoverable content survives; unterminated raw-text (script/style) swallows the tail exactly as HTML5 specifies. |
| a03-iframe-noise | 60 cross-origin iframes + srcdoc + hidden tracker | Iframes stripped, never fetched, src not echoed; host content intact. |
| a04-meta-refresh | 12-hop refresh chain | Pure pipeline cannot follow redirects by construction; directives stay in `<head>` and never enter output. |
| a05-huge-attributes | 2.4 MB single attribute values | Bounded time (37 ms), content survives, output proportional to content not to attribute mass. |
| a06-empty-edge | near-empty body | Clean empty output, no panic. |

## 4. Naming-rule enforcement

Banned-token grep over every artifact this work created (crates, fixtures,
docs, xtask, plans): **0 matches** across all six PRs. Upstream references
use only *web oracle alpha/beta/gamma* and the `<pkg-root>` placeholder.

## 5. Honest divergences

1. **Parser substrate.** The PRD planned an `html5ever` parser; the offline
   crate registry never had it. Production ships the PR-0-validated
   `quick-xml` HTML parser with lenient-HTML discipline (entity
   `GeneralRef` handling, void elements, sibling implicit-close, unmatched
   closer re-nesting, `MAX_PARSE_DEPTH`). Swapping to `html5ever` later is
   a contained change behind `dom.rs`.
2. **Computed-style count.** The PRD said "11 required computed styles";
   the pinned gamma table is 10 (display, visibility, opacity, overflow,
   overflow-x, overflow-y, cursor, pointer-events, position,
   background-color). The pinned source is authoritative; the count is
   documented in `snapshot.rs`.
3. **Converter.** `htmd`/`html2md` were also offline-unreachable; the
   committed converter is the PR-0 minimal markdown emitter whose
   byte-determinism is golden-pinned across 12 fixtures × 2 paths.
4. **Deep-nesting cap.** 5,200-deep chains flatten past `MAX_PARSE_DEPTH`
   rather than exhausting the stack — the same trade browsers make.
5. **Execution engines.** Tool *wiring* (PR-5) is complete and parity-
   proven; the sandbox-routed *engines* (fetch transport attaching to the
   tools, live CDP driver sessions) compose at the host boundary per the
   PRD's port design and fail closed with model-facing refusals until a
   host attaches them. The ports themselves are PR-3/PR-4-validated.
6. **Live-docker proof** is `#[ignore]`-gated (needs a daemon); the PR-0
   pipe-driver validation proved the CDP-over-anonymous-pipes channel and
   the kernel-level no-TCP/no-port facts against a real Chrome-family
   headless process.

## 6. Post-audit verification

```
cargo test --workspace --all-features      # 86 suites, 1646 passed, 0 failed, 1 ignored
cargo clippy --workspace --all-targets --all-features -- -D warnings   # clean
cargo run --package xtask --quiet -- architecture                        # 25 packages validated
cargo test --release -p vesper-web --test adversarial -- --ignored       # both perf gates green
```
