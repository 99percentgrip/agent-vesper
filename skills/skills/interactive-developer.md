---
name: interactive-developer
description: Rigorous human-in-the-loop state machine for generating any artifact (code, UI, scripts, configs) via VesperLens — interview with request_human_input, build with write_file, review with request_human_review, revise until explicit Approve. Never skip the review.
version: 1.0.0
author: Alex (owner-authored); Agent Vesper library
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [interactive, human-in-the-loop, vesperlens, artifact-generation, workflow]
---

# Interactive Developer

A strict state-machine workflow for generating any artifact through a
human-in-the-loop process using VesperLens tools.

## When to Use

Use this skill when the user wants to generate an artifact (e.g. a
dashboard, a script, a website, a CLI tool) and expects a robust,
interactive process where requirements are clarified upfront and the
output is explicitly reviewed before completion.

## The Workflow Rules (Strict State Machine)

You MUST follow these exact steps in order. Do not skip steps.

### Phase 1: The Interview (request_human_input)

- Do not start implementation immediately.
- First, use the `request_human_input` tool to open a VesperLens
  Interview.
- Ask targeted questions (multiple-choice or free-text) to clarify the
  artifact's requirements (e.g. styling, architecture, features, edge
  cases, naming).
- Wait for the user to submit their answers.

### Phase 2: The Build (write_file)

- After receiving the interview answers, generate the requested artifact.
- Use the `write_file` tool to save the artifact to disk.

### Phase 3: The Review (request_human_review)

- Immediately after writing the file, use the `request_human_review` tool.
- Wait for the human to evaluate the artifact in the VesperLens UI.
- The user will respond with one of three decisions: Approve, Send
  changes, or Reject.

### Phase 4: Revision Loop

- If the user selects "Send changes" or provides feedback, apply the
  requested revisions using `write_file`.
- After updating the file, immediately call `request_human_review` again.
- Repeat this loop until the user selects "Approve".

### Phase 5: Completion

- Do NOT declare the task complete until you receive an explicit
  "Approve" decision from the VesperLens review tool.
- Once approved, summarize the final artifact and conclude the turn.

## Gotchas

- Do not plan and yield without calling tools: You must actively execute
  the tools. When asked to generate the artifact, formulate the questions
  and call `request_human_input` in the same turn.
- Never bypass the review: Even if you are confident in the code, you must
  invoke `request_human_review` before finishing.
