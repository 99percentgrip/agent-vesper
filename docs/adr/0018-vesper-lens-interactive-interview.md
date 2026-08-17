# ADR 0018 — VesperLens Interactive Handoff and Planning Interview

- **Status:** Accepted
- **Date:** 2026-08-17
- **Builds on:** ADR 0017

## Context

ADR 0017 established a loopback-only, single-turn browser review seam, but the
first implementation still missed the user-visible interaction contract:

- the TUI announced a URL while the agent blocked instead of presenting the
  browser automatically;
- annotation capture started immediately and consumed artifact clicks, making
  interactive dashboards and prototypes unusable during review;
- `Action::Modify` existed in Rust but no browser action submitted it;
- planning had no structured browser answer channel comparable to the review
  workflow's input controls;
- every task-plan update was copied into chat history while the dedicated TODO
  surface had been removed.
- conversation turns still used wide iMessage-style background bubbles despite
  the stated Codex/Claude terminal hierarchy.

The comparison target's important behavior is the handoff and feedback loop,
not its Node process model or source code. Vesper remains a native Rust,
loopback-only implementation.

## Decision

1. The TUI opens each Lens URL with the platform browser immediately after the
   listener binds. The opener inherits no TUI stdio and is reaped off-thread.
   A bare URL, in-app click handling, and Ctrl+O remain fallback paths.
2. Artifact review starts in interaction mode. Annotation capture is opt-in,
   and the user can return to interaction mode without ending review.
3. The overlay exposes Approve, Send changes (`modify`), and Reject; preserves
   draft notes across rerenders; and rejects non-success HTTP responses.
4. `LensFeedback` gains optional `answers: Vec<LensAnswer>`. Existing payloads
   remain compatible through serde defaults.
5. `render_interview_artifact` builds a script-free escaped form from 1–4
   bounded `LensQuestion` values. `request_human_input` exposes that surface to
   the model, blocks through the existing `LensReviewPort`, requires every
   question to be answered, and returns stable question/value context.
6. Current TODO state is rendered in a dedicated toggleable sidebar panel.
   `PlanUpdated` mutates task state but no longer appends snapshots to chat.
   The former full-height Run gauge becomes a compact current/last-run panel.
7. Conversation turns use a terminal-native feed: cyan `›` for user prompts,
   unboxed assistant markdown, and dim inline thinking/tool telemetry. Legacy
   chat-bubble backgrounds are removed.

## Boundaries

- ADR 0017's loopback-only `127.0.0.1:0`, raw HTTP, 64 KiB request cap,
  single-turn listener, relative `/feedback`, and zero-new-dependency contracts
  remain unchanged.
- Interview titles, prompts, ids, and options are HTML escaped before rendering.
- The model may ask at most four questions with at most six options each.
- Browser-opening failure never loses the review URL or blocks manual recovery.
- The TUI does not claim full source parity with the comparison project; this
  decision covers the native interaction and planning-answer contracts only.

## Consequences

- HTML artifacts can be operated and annotated in the same review session.
- Ambiguous planning turns can pause for explicit human choices and continue
  from structured tool output instead of guessed requirements.
- The conversation remains conversational while the sidebar owns ephemeral
  plan state.
- The browser remains a single-turn surface; persistent multi-message review
  sessions and sibling-asset serving are separate future decisions.

## Verification

- `cargo test -p vesper-agent --lib planning::vesper_lens`
- `cargo test -p vesper-agent --lib vro::lens_integration`
- `cargo test -p agent-vesper-tui --lib`
- `cargo xtask architecture`
- `cargo clippy -p vesper-agent -p agent-vesper-tui --all-targets -- -D warnings`
