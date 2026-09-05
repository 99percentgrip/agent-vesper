# Web Oracle Fixture Corpus (VRO-14 PR-1)

## Purpose

Deterministic, offline HTML fixtures for the `vesper-web` perception
pipeline: every fixture is pure static HTML (no network, no scripts
executed, no credentials), exercising the Parse → Strip → Prune → Convert
stages.

## Provenance

Hand-authored for the PR-0 portability spike and carried into production
fixtures. Content is original synthetic text (no upstream content from the
web oracle triad repositories). Fixture names and categories:

- `f01-article` — long-form article with heavy header/nav/aside/footer
  boilerplate, tables, code blocks.
- `f02-docs` — reference docs (definition-style sections, code samples).
- `f03-landing` — marketing landing page (hero, feature grids).
- `f04-spa-shell` — empty client-rendered shell (noscript + scripts).
- `f05-forum` — forum thread (post list, nested quoting).
- `f06-table-heavy` — data page dominated by tables.
- `f07-api-docs` — endpoint reference with inline code and lists.
- `f08-wiki` — encyclopedic entry with infobox aside.
- `f09-nav-heavy` — settings page dominated by navigation chrome.
- `f10-listicle` — numbered list content.
- `f11-edge` — edge cases: entities, unicode, deep nesting, empty bodies,
  comments, `template` content, self-closing foreign elements.
- `f12-e-commerce` — product page (price, spec table, reviews).

## Local Contracts

- Fixtures are LF-only, UTF-8, and must never contain credentials or
  live-provider text.
- `goldens/` holds byte-identical expected markdown per fixture, produced
  by the `vesper-web` pipeline with default options; regeneration is a
  deliberate act (`cargo test -p vesper-web --test corpus_golden --
  --bless`), never accidental.
- The corpus is offline: no test in `vesper-web` may open a socket, file
  outside its own crate paths, or clock-dependent value.

## Verification

- `cargo test -p vesper-web` (unit + corpus golden tests).
