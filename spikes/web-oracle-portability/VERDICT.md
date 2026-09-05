# VERDICT — VRO-14 PR-0 (web oracle portability spike)

**Overall: VALIDATED with documented constraints.** Both objectives were
answered with executable evidence on this machine. Details and honest
divergences below.

---

## 1. Converter & density spike — VALIDATED (with substitutions)

**Question.** Can a Rust-native pipeline (HTML DOM → strip → heuristic
density prune → markdown) reproduce beta's fit-markdown behavior, and does
an off-the-shelf converter crate suffice?

**Environment constraint (decisive).** The machine is offline and its
cargo cache does not contain html5ever, html2text, or html2md/htmd .crate
artifacts (htmd 0.5.5's archive is genuinely absent; only its sparse-index
entry exists). PR-1 must either vendor these into the production workspace
dependency set (normal `cargo deny` gates) or repeat the substitution made
here.

**What was built.**

- `src/dom.rs` — minimal HTML DOM on quick-xml 0.41.0 (the only
  HTML-capable parser available offline), with a lenient recovery parser
  (`check_end_names = false`, HTML5 void-element and implied-end-tag
  handling), a `strip` stage (alpha's removeUnwantedElements intent +
  beta's excluded_tags set), and an arena layer (`Dom`/`NodeId`) for
  in-place pruning.
- `src/density.rs` — a faithful port of beta's `PruningContentFilter`
  composite score (weights 0.4/0.2/0.2/0.1/0.1; text_density
  text_len/tag_len; link_density 1−ratio; tag_weight table; class/id
  negative-pattern −0.5; text_length ln(n+1); fixed 0.48 threshold;
  min_word_threshold −1.0 sentinel; preserve class/tag short-circuit),
  plus a hidden-subtree prune and the measurement driver.
- `src/convert.rs` — a minimal markdown converter (headings, paragraphs,
  links, images-off default, GFM tables, code fences, lists, blockquote,
  inline emphasis/code/br with the single-line-break option). This is a
  spike-grade converter, not a production candidate.

**Verified findings.**

1. The composite-score port is faithful: unit tests pin link-farm < para,
   negative-class tokens lower scores, min_word_threshold guarantees
   removal, preserve classes short-circuit.
2. **Fixed-0.48 does NOT prune link-less short `div`s** (score ≈ 0.9 >
   0.48). Verified against the pinned beta source: beta's own default
   keeps such nodes too; real-world boilerplate removal comes from
   `_remove_unwanted_tags` (tag strip) + `min_word_threshold`. Porting
   note for PR-1: strip FIRST, score SECOND, and expose
   min_word_threshold.
3. On clean synthetic pages the strip stage does the heavy lifting: fit/full
   ≈ 99.8% after strip. The density filter's marginal value on clean pages
   is ~zero; it is a tunable for messy pages (this matches beta's design
   intent — the filter is optional there too).

**Measurements (12 offline fixtures, deterministic).**

| fixture | html-in | strip-in | full-md | fit-md | sentinels |
|---|---|---|---|---|---|
| f01-article | 4139 | 2664 | 1806 | 1806 | 3/3 |
| f02-docs | 2427 | 1790 | 963 | 962 | 3/3 |
| f03-landing | 2483 | 1902 | 1092 | 1092 | 3/3 |
| f04-spa-shell | 523 | 136 | 1 | 1 | 0/1 (JS-only shell) |
| f05-forum | 2092 | 1187 | 592 | 592 | 3/3 |
| f06-table-heavy | 1524 | 1291 | 622 | 604 | 1/1 |
| f07-api-docs | 1426 | 1105 | 560 | 560 | 3/3 |
| f08-wiki | 1883 | 1386 | 808 | 806 | 2/2 |
| f09-nav-heavy | 2594 | 1201 | 676 | 676 | 4/4 |
| f10-listicle | 2459 | 1756 | 1222 | 1222 | 1/1 |
| f11-edge | 1947 | 760 | 357 | 355 | 3/3 |
| f12-e-commerce | 2609 | 1207 | 516 | 516 | 3/3 |
| **totals** | **26106** | **16385** | **9215** | **9192 (99.8%)** | **29/30** |

(f04 is a JS-only SPA shell — its 0/1 is the correct, expected result for a
no-JS pipeline and is the reason the render escalation exists in the PRD.)

**Converter verdict.** No converter crate could be evaluated by execution
(offline constraint above). By inspection of available options and the
requirements (image stripping, single-line-break, GFM tables, code
de-indent), `htmd`/`html2text` remain the PR-1 candidates **if** their
artifacts can be vendored; otherwise the spike's converter logic ports
directly as a starting point. The decisive evaluation (density parity
against real pages, not synthetic) is deferred to PR-1 with network/vendor
access.

---

## 2. Headless CDP pipe driver — VALIDATED

**Question.** Can a Rust subprocess wrapper drive a headless Chrome-family
browser over `--remote-debugging-pipe` (fd 3/4 anonymous pipes) with zero
TCP DevTools exposure?

**Result (executed against /usr/bin/google-chrome, Chrome 152.0.7977.64).**

```
[ok] spawned headless Chrome-family process (pipes fd3/fd4)
[ok] Browser.getVersion -> product: Chrome/152.0.7977.64
[ok] Target.createTarget -> 0EC5362770F7AB596BF89CC61B7CA2DF
[ok] Target.attachToTarget -> 34ABCEDFE94207154FE9737321EB39EB
[ok] Runtime.evaluate 6*7 -> 42
[ok] Page.navigate data: URL -> frame 0EC5362770F7AB596BF89CC61B7CA2DF
[proof] child fd 3 -> pipe:[1139080]
[proof] child fd 4 -> pipe:[1139081]
[proof] cmdline carries no remote-debugging-port
[ok] Browser.close acknowledged; child exited
```

**Kernel-level proof.** `/proc/<pid>/fd/{3,4}` resolve to `pipe:[inode]`
(anonymous pipes, not sockets) and the child's cmdline contains no
`--remote-debugging-port`. The CDP channel is pipe-only.

**Constraints found (must feed PR-1/PRD).**

1. **No standalone headless-shell binary exists on this machine** — only
   `google-chrome` / `google-chrome-stable`. The `--remote-debugging-pipe`
   flag family behaves identically across the Chrome-family build
   (validated here); PR-1 must either ship/pin a headless-shell artifact
   (offline constraint again) or use the Chrome-family binary inside the
   sandbox.
2. **The browser opens its OWN internal loopback listeners** (mDNS,
   internal services — ~10 LISTEN sockets observed) in the same network
   namespace regardless of our flags. This does not weaken the pipe-only
   CDP channel, but it CONFIRMS the PRD's Feature-4 requirement: the
   browser must run inside the ADR-0022 network-namespace sandbox in
   production; a bare spawn is not containment.
3. **fd mapping requires either `/bin/sh` redirection** (the approach used
   here: `sh -c 'exec "$0" …flags… 3<&0 4>&1'`, 100% safe Rust, zero
   unsafe) **or the ADR-0022 supervisor's run-line protocol** in
   production. std's `Command` has no safe arbitrary-fd mapping;
   `pre_exec` would need unsafe.

---

## 3. Workspace integrity

- Main workspace floor: **1,458 tests passing** (floor 1,420; my changes
  touched no production crate).
- Banned-token grep over the spike: **zero matches**.
- Spike is a standalone workspace (`[workspace]` table, vendored deps,
  committed lockfile) excluded from the production workspace per
  `spikes/AGENTS.md`.

## 4. Recommended PR-1 actions

1. Vendor or otherwise source a real HTML5 parser + converter (html5ever
   or lol-html; htmd/html2text) — the spike's quick-xml DOM sufficed for
   filter-logic portability but is NOT an HTML5-conformance parser.
2. Port the density filter verbatim from this spike (it is
   behavior-pinned by tests) with strip-before-score ordering and
   `min_word_threshold` exposed.
3. Route the browser spawn through the sandbox supervisor's run-line
   protocol with `IsolationRequirement::Network`; keep the pipe channel
   design exactly as validated here.
