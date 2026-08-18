# Seed skill library

## Purpose

Own the bundled seed skill library shipped inside release archives and
seeded into `~/.agent-vesper/memory/` by the installers. A fresh install
must land with the full curated suite; existing learned skills are never
overwritten or resurrected.

## Ownership

- `skills/` — mirrors the global memory root layout: one `<slug>.md` per
  skill plus an optional `<slug>/` resource directory.
- `bundles/` — category bundles (`<name>.json`) referencing seed slugs.
- Verified by `skills` counts below; content provenance is the curated
  external library, rewritten Agent Vesper-native (no foreign harness
  references, absolute resource paths under `~/.agent-vesper/`).

## Local Contracts

- Every `.md` file must have a valid slug stem and YAML frontmatter with
  `name` and `description` (the `list_skills` headline, Oracle format
  `- {name}: {description}`).
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
  bundle skill references (currently 82).
- A case-insensitive grep for the foreign harness name over
  `skills/` returns no matches (kept verbatim-free here so this gate is
  self-checking).
- `cargo xtask verify` stays green after any change touching packaging.

## Child DOX Index

No children.
