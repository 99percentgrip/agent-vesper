---
name: session-librarian
description: "Organize sessions by prompt: find, rename, back up, prune."
version: 1.1.0
author: Agent Vesper library + Teknium
license: MIT
platforms: [linux, macos, windows]
tags: [sessions, organization, cleanup, library, productivity]
related_skills: [weekly-review-planning]
---

# Session Librarian

Manage the user's session library conversationally: find past sessions about a
topic, summarize what they decided, rename them meaningfully, split work into
parallel sessions, and propose stale ones for cleanup — all from a
plain-language request like *"find my sessions about Q3 pricing, keep the
useful ones, and clean up the duplicates."*

Always show the plan before touching anything.

## When to Use

- "What sessions do I have about X?" / "What did we decide about X?"
- "Rename these sessions to something meaningful."
- "Clean up my session library" / "remove the stale ones."
- "Fork that session into a follow-up focused on Y."
- "Split this into one session per ticket" (see Parallel workstreams below).

## The Surfaces

| Task | Surface |
|---|---|
| Find sessions by topic, read content, summarize decisions | `session_search` tool (FTS over the persisted session store) |
| List sessions in this project | `/sessions` slash command |
| Start a fresh session | `/sessions-new` |
| Rename the current session | `/rename <title>` |
| Fork the current session | `/branch` |
| Export a transcript before touching anything valuable | `/export` |
| Inspect stored session files directly | `bash` + `list_directory` under `.agent-vesper/sessions/` (project-local) |

## Procedure

① **Discover.** Use `session_search(query=..., limit=5-10)` with topic
keywords; vary phrasing (feature name, symptom, project name). For a
project-wide listing, use `/sessions`.

② **Summarize per session.** The match window plus each session's opening
goal and closing outcome usually suffice — only dump a full session when the
user asks for decisions in depth. Report each as: session id — one-line goal —
one-line outcome.

③ **Plan before acting (MANDATORY for anything that mutates).** Present a
plan table first: which sessions get renamed to what, which are proposed for
deletion and why (duplicate of which keeper, stale, empty). Wait for the
user's go-ahead. Exception: a single rename the user explicitly dictated can
be done directly.

④ **Act with the safest primitive.**
- Prefer reversible actions: rename (`/rename`) and export (`/export`) are
  always safe.
- Session files live under the project's `.agent-vesper/sessions/` directory;
  propose deletions as explicit `bash` commands and run them only after the
  user confirms the plan in this conversation.
- Before deleting anything with meaningful content, offer `/export` as a
  backup.

⑤ **Report.** Renames applied, sessions exported, sessions deleted (count),
anything skipped and why.

## Parallel Workstreams

For "one session per ticket, investigate each, report back": use
`delegate_task` with one bounded task per workstream — each delegation runs
independently — then synthesize their summaries.

## Pitfalls

- **Never delete without explicit confirmation in this conversation.** A
  standing "clean things up" is authority to *propose*, not to delete.
- **`session_search` finds content, not metadata.** Age/size filters require
  inspecting files under `.agent-vesper/sessions/` via `bash`.
- **Titles are identity for `/rename`.** Keep titles short, unique, and
  prefix-friendly.
- **Sessions are project-local.** There is no cross-project session registry
  in Agent Vesper; say so when the user asks for one.

## Verification

After a cleanup pass, re-run the discovery query and `/sessions` to confirm
the library reflects the plan.
