# ADR 0020: Isolated, authenticated, resumable VesperLens review

## Status

Accepted. Supersedes ADR 0017 and ADR 0018 only where they require a
same-document overlay, unauthenticated single-turn feedback, no sibling assets,
or a dormant `VroOrchestrator` final-output review seam. ADR 0019's interview
limit policy remains active.

## Context

The first VesperLens implementation proved the explicit browser interview and
review flow, but a comparison against the authorized Lavish reference exposed
four production gaps:

- artifact JavaScript shared an origin and document with trusted review controls;
- `POST /feedback` had no session authentication, Host guard, or origin guard;
- an interrupted tool wait lost its browser session and relative assets returned 404;
- annotation targets and interview metadata were too shallow for precise iteration.

The inactive `VroOrchestrator::maybe_review_html_artifact` seam also contradicted
the explicit-tool decision and had no production caller.

## Decision

1. The top-level document is trusted VesperLens chrome. The artifact runs in an
   iframe sandboxed with `allow-scripts allow-forms allow-popups allow-downloads`
   and never receives `allow-same-origin` or top-navigation authority.
2. The artifact receives only an owned annotation SDK. It may send typed
   `postMessage` observations but cannot submit feedback. Approve, Modify,
   Reject, and End session exist only in trusted chrome.
3. Every session uses a UUIDv4 route token. Feedback additionally requires the
   exact loopback Host, exact same Origin, `application/json`, and a matching
   `X-Vesper-Lens-Token` header. Trusted chrome has a restrictive CSP.
4. Model-provided review paths are confined to the active workspace, restricted
   to `.html`/`.htm`, canonicalized through symlinks, and bounded to 8 MiB.
   Sibling assets are bounded to 16 MiB and must remain beneath the artifact
   directory after canonicalization.
5. File sessions are keyed by canonical path and remain owned by the shared
   `VesperLens` instance. Feedback is queued independently of an individual tool
   future, repeated rounds reuse the same URL, and browser drafts persist across
   reloads in session storage. File revisions are polled for live reload.
6. Annotations carry stable IDs and typed element or text-range targets with
   range paths and offsets. Comments and suggested HTML are editable, and
   removal clears the artifact highlight.
7. Planning questions may include descriptions, optionality, recommendations,
   and an Other value while retaining ADR 0019 limits and escaping.
8. `request_human_review` is HTML-only and conditional: use it for requested or
   materially useful visual inspection, not for ordinary source code or fully
   specified HTML that deterministic checks can validate.
9. The unused VRO final-output interception seam is removed. Explicit
   `request_human_review` and `request_human_input` remain the only triggers.
10. The sandbox SDK reports bounded horizontal-overflow and clipped-content
    diagnostics. They remain passive browser warnings and enter feedback only
    when the reviewer explicitly selects them.

## Consequences

- Artifact code cannot forge a human verdict through same-document access.
- Cancelled tool waits do not discard already-running browser sessions or queued
  feedback while the TUI process remains alive.
- Complex artifacts load local CSS, JavaScript, images, and fonts without
  granting directory traversal or symlink escape.
- Layout diagnostics cannot wake the agent or manufacture feedback; they are
  suggestions the reviewer may confirm and include.
- Review remains native Rust with no web framework. The existing workspace UUID
  dependency is added to `vesper-agent`; no new third-party package is introduced.
- A full detached daemon surviving host-process termination remains outside this
  in-process native session contract; browser state and submitted feedback are
  resilient to reload and tool cancellation, not machine or process shutdown.

## Verification

- `cargo test -p vesper-agent planning::vesper_lens`
- `cargo test -p vesper-agent vro::lens_integration`
- `cargo test -p agent-vesper-tui request_human`
- `cargo clippy -p vesper-agent -p agent-vesper-tui --all-targets -- -D warnings`
- `PLAYWRIGHT_MODULE=<playwright-package> node crates/vesper-agent/tests/vesper_lens_browser.mjs`
- `PLAYWRIGHT_MODULE=<playwright-package> node crates/vesper-agent/tests/vesper_lens_interview_browser.mjs`
- `cargo xtask verify`

The Playwright tests drive real Chrome. The artifact flow verifies sandbox
flags, sibling CSS and JavaScript, interaction, editable annotations, and
approval. The interview flow verifies mode-specific controls, required-answer
validation, and structured answers. Both fail on console or request errors.
