---
name: vesper-skill-authoring
description: "Author high-quality skills for Agent Vesper: format, tools, bounds."
version: 1.0.0
author: Agent Vesper library
license: MIT
platforms: [linux, macos, windows]
tags: [skills, authoring, documentation, quality]
related_skills: [plan, requesting-code-review]
---

# Vesper Skill Authoring

Write and refine skills for the Agent Vesper learned-skill library. A skill is
the highest-leverage quality artifact the agent owns: a good skill prevents
whole classes of mistakes; a bloated one wastes context and misleads.

## Where skills live

- **Project-local (writable):** `.agent-vesper/memory/skills/<slug>.md` —
  created by `learn_skill`, listed by `/skills`. Learned skills belong here.
- **Cross-project library (read layer):**
  `~/.agent-vesper/memory/skills/<slug>.md` — curated reference skills
  available in every project. Local skills shadow global ones with the same
  slug. Resource directories (`references/`, `scripts/`, `templates/`) sit
  next to the skill file at `<root>/skills/<slug>/`.

## File format

```markdown
---
name: <slug>
description: "<one-line trigger: what this does + when to use it>"
version: 1.0.0
author: <attribution>
license: MIT
platforms: [linux, macos, windows]
tags: [<lowercase tags>]
related_skills: [<existing slugs only>]
---

# <Title>

## When to Use
## Procedure
## Pitfalls
## Verification
```

The `description` is what `list_skills` surfaces — write it as the trigger
condition ("Use when the user asks for X"), not a table of contents.

## Authoring tools

- `learn_skill(name, description, instructions)` — writes the file; optional
  `environments`, `requires_tools`, `tasks` arrays land in frontmatter.
  Description ≤ 500 chars, instructions ≤ 12,000 chars.
- `read_skill(name, section?, offset?, limit?)` — `section` returns one
  heading's slice; `offset`/`limit` window large skills instead of loading
  them whole.
- `list_skills`, `manage_skill` (pin/unpin/archive/restore),
  `manage_skill_bundle` (curated groups), `evolve_skill` (candidate →
  promote).

## Quality rules

- Concise procedures and pitfalls only. Never credentials, raw reasoning,
  transient state, or routine steps.
- Every command must be real: verify tool names against the hosted surface
  (`bash`, `read_file`, `write_file`, `grep`, `list_directory`,
  `search_files`, `web_search`, `web_reader`, `session_search`,
  `delegate_task`, `worktree_worker`, `cronjob`, `apply_patch_set`, ...)
  before shipping a skill that names them.
- State absolute paths for skill resources
  (`~/.agent-vesper/memory/skills/<slug>/references/...`).
- Slugs: lowercase `[a-z0-9-]`, ≤ 64 chars. Bodies ≤ 200 KB — a skill that
  needs the ceiling is usually several skills.
- Prefer one focused skill over a mega-skill; bundles group them.

## Verification

After authoring: `read_skill` the result, confirm the description surfaces in
`list_skills`, and dry-run the procedure's first step in a throwaway context.
