# Seed skill library

## Purpose

Own the bundled seed skill library shipped inside release archives and
seeded into `~/.agent-vesper/memory/` by the installers. A fresh install
must land with the full curated suite; existing learned skills are never
overwritten or resurrected.

## Ownership

- `skills/` — mirrors the global memory root layout: one `<slug>.md` per
  skill plus an optional `<slug>/` resource directory. Ships the
  `work-unit-reporting` methodology skill; the binding mandate it
  implements is the shared `COMPLETION_REPORTING_INSTRUCTION` in
  `vesper-harness`, injected by both hosts.
- `bundles/` — category bundles (`<name>.json`) referencing seed slugs.
- Verified by `skills` counts below; content provenance is the curated
  external library, rewritten Agent Vesper-native (no foreign harness
  references, absolute resource paths under `~/.agent-vesper/`).

## Local Contracts

- Every `.md` file must have a valid slug stem and YAML frontmatter with
  `name` and `description` (the `list_skills` headline, Oracle format
  `- {name}: {description}`).
- Optional on-demand chunk tier (`chunks/`): a skill declares a nested
  `chunks:` frontmatter block; chunk files live at
  `<root>/skills/<slug>/chunks/<name>.md`. Contract:
  - Manifest entry: `name` (valid slug, unique per skill, required) +
    `description` (required, ≤ 240 chars) + optional `summary`
    (≤ 480 chars) + optional `key_elements` (≤ 8 entries).
  - Caps: ≤ 32 chunks per skill, ≤ 24,000 bytes per chunk file
    (`MAX_CHUNKS_PER_SKILL` / `MAX_CHUNK_BYTES`).
  - Fail-closed: oversize/missing/malformed/duplicate manifests make the
    skill routing-ineligible with an explicit reason — never silently
    truncated. Chunk loads count against the per-skill 24 K and total
    60 K context budgets.
  - Routing (post PR-4 ADOPT, 2026-09-12): automatic chunk routing feeds
    on `description` + `summary` + `key_elements`; at most 3 chunks load
    per selected skill and zero-overlap chunks never load.
  - When chunks beat one body: use them when knowledge density requires
    bounding the context — reference material an author would otherwise
    have to shrink or split into multiple skills (per-topic lookup
    references, long procedures, or any content the user asks about
    selectively). Keep one focused body when the whole skill is needed on
    every activation; chunks are for selectively-read knowledge.
- Bodies must not reference foreign harness state directories or their
  environment variables; resource paths must be absolute under
  `~/.agent-vesper/memory/skills/<slug>/`.
- Bundle files must reference only slugs present in `skills/`.
- Body size ≤ 200 KB (`vesper-memory` `MAX_SKILL_BYTES`); file count ≤ 500
  (`MAX_SKILL_FILES`).
- Changes here require the matching installer-seed behavior check in
  `scripts/AGENTS.md` and a release-notes line.

## Work Guidance

- Add or update a seed skill: edit here, then mirror to
  `~/.agent-vesper/memory/skills/` for immediate local use (installers
  only seed fresh homes or manifest-unseen slugs).
- Never store secrets, credentials, or project-transient state here.

## Verification

- `find skills/skills -maxdepth 1 -name '*.md' | wc -l` equals the sum of
  bundle skill references (currently 95).
- A case-insensitive grep for the foreign harness name over
  `skills/` returns no matches (kept verbatim-free here so this gate is
  self-checking).
- `cargo xtask verify` stays green after any change touching packaging.

## Child DOX Index

No children.
