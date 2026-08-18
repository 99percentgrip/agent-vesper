---
name: agent-vesper
description: Use, configure, and orchestrate the Agent Vesper coding agent (TUI, ACP, memory, skills, reasoning).
version: 1.0.0
author: Agent Vesper library
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [agent, configuration, memory, skills, reasoning, acp]
prerequisites:
  commands: [agent-vesper-tui]
---

# Agent Vesper

Agent Vesper is a Rust-native coding-agent harness: a terminal UI, an ACP
stdio server for editor integration, durable memory, learned skills, and
the Vesper Reasoning Orchestrator (VRO).

## Binaries

- `agent-vesper-tui` — interactive terminal UI (default way to work).
- `agent-vesper-acp` — ACP v1 stdio server for Zed and other
  ACP-compatible editors. Optional non-interactive setup:
  `agent-vesper-acp --setup`.

## Authentication

- Provider-routed: the active provider advertises its auth method; nothing
  is hardcoded to one vendor.
- Production registers Z.ai GLM. Set `ZAI_API_KEY` or use the TUI `/auth`
  command (re-opens the provider's authentication flow).
- Credentials are stored per provider and account via the OS credential
  manager with an owner-only vault fallback.

## Memory and skills layout

- Project state: `<project>/.agent-vesper/` (per-project memory, sessions,
  checkpoints, cognition).
- Cross-project global memory root: `~/.agent-vesper/memory/`
  (override with `AGENT_VESPER_GLOBAL_MEMORY_ROOT`). Global skills are
  appended after project-local ones; local slugs shadow global ones.
- Skills live at `<root>/skills/<slug>.md`; resource directories sit next
  to the file at `<root>/skills/<slug>/`. Bundles (named skill groups) are
  `<root>/bundles/<name>.json`.

## Core slash commands (TUI)

- `/skills` — list learned skills (headline = frontmatter description).
- `/memory`, `/goal`, `/subgoal`, `/profile`, `/awareness`,
  `/deliberation`, `/metacognition` — durable memory surfaces.
- `/reasoning` — VRO orchestrator override (off by default).
- `/thinking` — provider reasoning-depth superpower (distinct from VRO).
- `/auth` — re-run provider authentication.
- `/model` — provider-routed model selection.
- `/checkpoint`, `/rollback` — explicit workspace snapshots (never auto).
- `/help` — full command list.

## Reasoning

- VRO (`/reasoning`) orchestrates multi-strategy reasoning turns
  (decomposition, verification, tree search, replay).
- `/thinking` selects the provider's reasoning depth — a provider
  superpower, not an orchestrator mode.

## Human review

- VesperLens: the agent requests structured human input/review via
  `request_human_input` / `request_human_review`; feedback returns to the
  same agent turn.

## Agent tools for this store

`list_skills`, `read_skill` (supports a `section` argument),
`learn_skill` (only after a verified outcome), `forget_skill`,
`list_skill_bundles`, `read_skill_bundle`, `manage_skill_bundle`.

## Notes

- Sessions are transactional; workspace checkpoints are explicit-only.
- No auto-snapshotting, no background state writes during verification.
