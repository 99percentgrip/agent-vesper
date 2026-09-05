# web-oracle-portability spike

## Purpose

Validate the VRO-14 PR-0 portability assumptions offline: (1) the
DOM-to-Markdown pipeline (parser + strip + beta-port pruning filter +
minimal converter) on 12 deterministic local fixtures, and (2) the
headless CDP-over-pipe channel (fd3/fd4 anonymous pipes, no TCP DevTools
endpoint) against the local Chrome-family binary.

## Ownership

- `src/dom.rs` — lenient quick-xml HTML5-ish parser (void elements,
  implicit-close rules, mismatched-end recovery, `check_end_names=false`),
  strip stage, tree/arena dual surface, `surviving_elements` (body-scoped).
- `src/density.rs` — beta's `PruningContentFilter` port (weights, dynamic
  threshold modifiers, negative class/id tokens, `min_word_threshold`,
  preserve lists), `prune_hidden`, fixture sentinels, `run` measurement
  driver, `Totals`.
- `src/convert.rs` — minimal markdown converter (ATX headings, inline
  emphasis/code, links, GFM tables, lists, fenced code, `br`,
  `single_line_break`, `ignore_images`, deterministic output).
- `src/cdp.rs` — `PipeCdp`: safe fd3/fd4 mapping via `/bin/sh` exec
  redirection (no unsafe), NUL-delimited CDP JSON framing, flat-session
  calls, graceful `Browser.close`; `no_tcp_proof` reads `/proc/<pid>/fd/3,4`
  (must be `pipe:[inode]`) and `/proc/<pid>/cmdline` (no
  `remote-debugging-port`).
- `fixtures/*.html` — 12 offline deterministic pages (article, docs,
  landing, SPA shell, forum, tables, API docs, wiki, nav-heavy, listicle,
  edge cases, e-commerce). No live network anywhere.
- `VERDICT.md` — spike verdicts, chosen converter decision, density
  measurement table, CDP evidence, divergences.

## Local Contracts

- Offline-first: all dependencies are vendored under `vendor/` (no network
  fetch at build time; `.cargo/config.toml` pins the vendored source).
- No production crate may depend on this spike; it declares its own
  `[workspace]` and stays outside the main workspace (`exclude`).
- Naming rule: upstream references use only the alpha/beta/gamma oracle
  names; no brand tokens anywhere in this directory.
- The CDP probe spawns the local Chrome-family binary with
  `--headless=new --remote-debugging-pipe --no-sandbox --disable-gpu`;
  the machine has no standalone headless-shell binary, so the Chrome-family
  substitution is documented rather than hidden.

## Work Guidance

- `cargo run --bin web-oracle-portability -- density fixtures` prints the
  byte-in/byte-out table and sentinel retention.
- `cargo run --bin web-oracle-portability -- cdp /usr/bin/google-chrome`
  runs the pipe handshake + kernel-level proof.
- `cargo test --locked` must stay green (13 tests).

## Verification

- `cargo test --locked` (13 passing).
- Banned-token grep over this directory returns zero matches.
- Main workspace floor untouched: 1,458 passing (floor 1,420).

## Child DOX Index

No children.
