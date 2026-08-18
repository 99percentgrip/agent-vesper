---
name: vesper-agent-skill-authoring
description: Author and maintain Agent Vesper skills (SKILL.md format, frontmatter, resources, bundles, quality gates).
version: 1.0.0
author: Agent Vesper library
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [skills, authoring, documentation, quality]
---

# Authoring Agent Vesper Skills

A skill is a markdown procedure the agent loads on demand. Skill quality
directly drives agent output quality — write skills as tight, factual,
verifiable procedures, never as prose essays.

## File layout

- One skill: `<memory-root>/skills/<slug>.md`.
- Slug rules: lowercase ASCII letters, digits, hyphens; must start and end
  with a letter or digit; max 64 chars.
- Optional resources: `<memory-root>/skills/<slug>/` directory next to the
  file (`references/`, `scripts/`, `templates/`). Reference resources with
  the absolute path `~/.agent-vesper/memory/skills/<slug>/...` so they
  resolve regardless of the working directory.
- Bundles: `<memory-root>/bundles/<name>.json` naming 1-32 skill slugs
  plus one activation instruction.

## Frontmatter (required)

    ---
    name: <slug>
    description: One factual line; becomes the list headline.
    version: 1.0.0
    author: Agent Vesper library
    license: MIT
    platforms: [linux, macos, windows]
    metadata:
      vesper:
        tags: [max, 8, tags]
    prerequisites:
      env_vars: [OPTIONAL_ENV_VARS]
      commands: [optional, external, clis]
    ---

The `description` is what `list_skills` shows (format
`- {name}: {description}`); make it the trigger condition plus the
outcome, not a title.

## Body structure (in order)

1. `# Title` matching the skill's purpose.
2. When to use / when NOT to use (explicit boundaries prevent misfires).
3. Procedure as numbered imperative steps — one action per step.
4. Verification section: the exact command(s) proving success.
5. Failure modes: what to check when verification fails.

## Quality rules

- Bound every command; state prerequisites honestly (missing tools must
  fail truthfully — never simulate or stub them).
- Reference only real capabilities of this agent; never describe
  behaviors of other harnesses or invent provider features.
- Keep the body focused; use the resource directory for large references
  and let the agent read them via `read_file` when needed.
- Hard bounds: skill body <= 200 KB, <= 500 skill files per store. Large
  skills should expose section headings so `read_skill(section: ...)`
  can fetch one part.

## Learning a skill at runtime

The agent's `learn_skill` tool persists a skill only after a successful
verification in the current task. Prefer refining an existing skill over
duplicating; retire stale skills with `forget_skill`.

## Review checklist before shipping a skill

- [ ] Slug valid; description is trigger + outcome.
- [ ] Steps are imperative and ordered; each is executable as written.
- [ ] Verification commands are real and copy-pasteable.
- [ ] Prerequisites (env vars, CLIs) declared in frontmatter.
- [ ] Resource paths are absolute under the skill's own directory.
- [ ] No references to foreign harnesses or their state directories.
