# Memory routing language assets

## Purpose

Own bounded, embedded language data used by Enhanced skill routing.

## Ownership

- `routing-verbs.txt` contains the sorted single-word ASCII verb lemmas derived
  from WordNet 3.0 `dict/index.verb`, with source archive hash and full license.

## Local Contracts

- Preserve the source/license header in every derived or distributed copy.
- No skill names, task fixtures, bodies or user data belong in this lexicon.
- Changes require a reproducible source derivation, not additions to fit failures.
- The runtime treats verb recognition only as a relevance hint, never permission.

## Work Guidance

- Keep source provenance and deterministic extraction documented in the owning report.

## Verification

- `cargo test -p vesper-memory --test routing_quality` checks lexicon ordering,
  general request verbs and conversational exclusions through the real selector.

## Child DOX Index

No children.
