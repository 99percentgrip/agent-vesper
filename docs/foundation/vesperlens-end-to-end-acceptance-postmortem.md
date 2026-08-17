# VesperLens End-to-End Acceptance Post-Mortem

## Objective

Record the owner-directed audit of the VesperLens implementation chain
(v0.20.29 → v0.20.44), the completion standard it established, and the ten
gaps it identified, so no future feature ships against the wrong standard.

## The completion standard (binding)

A user-facing feature is complete only when:

> The model can invoke it, the browser can interact with it, the user can
> submit, the feedback returns to the same agent workflow, and the process
> is protected by real integration tests.

Compile, unit tests, `cargo xtask verify`, and CI green are entry criteria,
never completion evidence. The v0.20.29 → v0.20.41 release chain — each
release fixing what the next live test exposed — is the signature of the
wrong standard: components shipped as products while the real TUI never
advertised, invoked, waited for, displayed, or returned the loop.

## Root failure modes (owner audit)

1. **Library, not product path.** The first implementation (commit `f04a7c0`)
   built the HTTP parser, server, overlay, types, and tests without connecting
   them to the native TUI's actual tool service. Completion was declared when
   internal components compiled.
2. **Dormant orchestrator seam.** Commit `2c690bd` added `with_lens_port`,
   `maybe_review_html_artifact`, and `LensReviewPort` with no production
   caller; the default `None` created a silent bypass
   (`docs/adr/0020-vesper-lens-isolated-resumable-review.md` records this).
3. **Implicit interceptor instead of an explicit tool.** Commit `0a18732`
   intercepted successful `write_file` calls on one ReAct route only; other
   strategies bypassed it. Removed; explicit tools are the only triggers.
4. **Review confused with planning.** No `request_human_input` existed until
   v0.20.42; a review overlay is not an interview system.
5. **Unenforced tool use.** A 180-second zero-tool turn (model announced a
   plan, called nothing) exposed prompts that failed to mandate execution.
6. **Component tests instead of workflow tests.** Parsing and string-injection
   proofs let missing submit actions, unopenable URLs, wiped overlays, and
   dead controls survive into releases.
7. **Untested interaction-model changes.** Removing the TODO surface, a
   dashboard-style split layout, duplicated review URLs, broken wheel
   scrolling, annotation-first blocking, and TODO snapshots polluting chat
   history were all regressions shipped without live testing.
8. **Unsafe same-document review.** Artifact JavaScript sharing the artifact
   document with review controls could remove, manipulate, or forge them.
9. **Copied visuals, missed lifecycle.** Canonical-path sessions, queued
   feedback, relative assets, reload state, and guarded routes were absent
   because the study focused on the visible overlay, not the
   open → wait → feedback → revise protocol of the Oracle reference.
10. **Premature completion claims.** Repeated releases fixing live-test
    failures indicate acceptance was "code compiles and unit tests pass."

## The ten audited gaps and corrections (v0.20.44)

| Gap | Previous problem | Correction |
|-----|------------------|------------|
| Isolation | Artifact and controls shared a document | Sandboxed iframe, trusted outer chrome |
| Submission authority | Artifact could influence verdicts | Only outer chrome submits feedback |
| Authentication | Unguarded feedback endpoint | UUID token, Host, Origin, content-type, custom-header checks |
| File confinement | Incomplete boundaries | Canonical workspace confinement, extension and size limits |
| Session lifecycle | Cancelled wait lost the session | Shared file sessions, queued feedback |
| Annotation precision | Shallow selector/comment | Stable IDs, element targets, exact text ranges |
| Interview richness | Basic questions only | Optional, descriptions, recommendations, Other values |
| Triggering | Implicit or excessive review | Explicit, conditional HTML-only review |
| Dead architecture | Unused interception seam | Removed; explicit tools only |
| Diagnostics | Layout findings confused with feedback | Passive, bounded, reviewer-selected warnings |

Decisions are recorded in `docs/adr/0020-vesper-lens-isolated-resumable-review.md`.

## Current implementation anchors

- `request_human_input` — structured interview tool; default maximum 4
  questions, fixed range 1–12; free-text, single-choice, multiple-choice,
  optional, help text, recommended answers, Other values, stable question
  IDs. Definition: `apps/agent-vesper-tui/src/main.rs`
  (`request_human_input_definition`); policy:
  `apps/agent-vesper-tui/src/commands.rs` (interview question limit).
- `request_human_review` — workspace-confined `.html`/`.htm` review;
  auto-opens the browser, blocks until feedback, returns approval,
  rejection, notes, and annotations. Production executor:
  `apps/agent-vesper-tui/src/main.rs`.
- Browser security — loopback-only binding, random session routes, exact
  Host/Origin validation, feedback token header, restrictive CSP, sandboxed
  artifact iframe, workspace and symlink confinement, 8 MiB artifact and
  16 MiB sibling-asset limits. Boundary:
  `crates/vesper-agent/src/planning/vesper_lens/server.rs`.
- Iteration — sessions keyed by canonical file path (URL reuse on reopen),
  feedback queued across cancelled waits while the TUI lives, drafts/answers/
  notes/scroll in tab session storage.
- TUI model — single-column conversation with inline thinking and tool
  telemetry plus a dedicated TODO sidebar (`render_sidebar` in
  `apps/agent-vesper-tui/src/ui.rs`); plan state stays out of chat history.

## Methods and evidence

- Owner live-testing of installed releases across v0.20.29 → v0.20.44.
- Owner-authored implementation report (2026-08-17) with commit-level
  attribution (`f04a7c0`, `2c690bd`, `0a18732`).
- Local re-verification on HEAD `0fcead7` (v0.20.44, clean tree):
  `cargo test -p vesper-agent vesper_lens` → 38 passed / 0 failed;
  `cargo test -p agent-vesper-tui --all-features` → 283 passed / 0 failed.
- Five-platform foundation workflow passed (Linux x86_64/ARM64, macOS
  Intel/Apple Silicon, Windows x86_64).

## Unresolved issues

- The two real-browser scripts (Chrome artifact review; Chrome interview and
  submission) pass when run manually but are **not** wired into
  `cargo xtask verify` or GitHub Actions. Until automated, they are manual
  checks, not a gate. Wiring them is the next implementation task.
- VesperLens is composed into the native TUI tool service only; it is not
  automatically available through every generic ACP host.

## Intentionally not implemented (never advertise)

Detached background server surviving TUI shutdown; disk-persisted sessions
across process restarts; browser conversation history and agent-presence
states; full durable layout-warning inbox; Mermaid-to-Excalidraw
whiteboards; HTML export with inlined assets; external sharing/publishing;
multi-reviewer handoff.

## Readiness effect

Establishes the binding acceptance standard for all future user-facing
features and the honest-scope list for VesperLens.

## Status

Accepted as the project completion standard; encoded as the project skill
`end-to-end-workflow-acceptance`.
