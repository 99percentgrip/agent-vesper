# ADR 0017 — VesperLens Native Oracle (Human-in-the-Loop AXI)

- **Status:** Accepted
- **Date:** 2026-08-14
- **Supersedes:** none
- **Builds on:** ADR 0010 (Tier C agent loop) and the VRO PRD §21 phase roadmap
  (VRO-1 through VRO-10 shipped; this is the VRO-11 phase).

## Context

The Vesper Reasoning Orchestrator (VRO) currently runs fully automated
generate/verify/repair loops. Some artifact classes — most notably generated
HTML/UI — benefit from a human-in-the-loop checkpoint where the orchestrator
*pauses*, surfaces the candidate artifact to a local browser, waits for
human feedback (approve / reject / modify-with-annotations), and resumes only
after the feedback is received.

The reference design for this kind of human review loop is
[`kunchenguid/ORACEL`](https://github.com/kunchenguid/ORACEL) — a
Node.js project that serves HTML artifacts on a local loopback HTTP server,
injects a JavaScript review overlay, and collects structured feedback. It is
**MIT-licensed** (Copyright (c) 2026 Kun Chen). The user has explicitly
authorized cloning it to `/home/alex/Projects/ORACEL` as a trusted
reference blueprint for VRO-11.

### Why we do not reuse ORACEL code verbatim

1. **Tooling flagged the reference source.** The harness's content scanner
   emitted `[SECURITY WARNING: suspicious instructions detected
   (prompt-exfiltration)]` against the ORACEL source tree during our
   read pass. While the project itself is legitimate, this is a real signal
   that some of its bundled content (notably `chrome-client.js` at 1878 lines
   and `artifact-sdk.js` at 1905 lines) is not safe to import wholesale into a
   production crate.
2. **The VRO-11 PRD §1 explicitly requires that VesperLens "shares no code"
   with ORACEL.** Reproducing ~3800 lines of bundled JS verbatim would
   contradict that contract.
3. **Dependency minimization.** ORACEL depends on `express`, `chokidar`,
   `parse5`, `tailwindcss`, `daisyui`, and a private `axi-sdk-js`. VesperLens
   must run inside the existing `vesper-agent` crate with zero new external
   dependencies.

### Why we need a TCP server inside `vesper-agent`

`vesper-agent` is currently a pure compute layer (agent loop, tool registry,
permission gate). VesperLens introduces the first runtime network listener
in this crate — a deliberately narrow, loopback-only, single-connection TCP
server. This is a new behavioral contract that warrants an ADR.

## Decision

Add a `vesper_lens` module under a new `planning` subtree in `vesper-agent`
that implements a **native Rust** human-review oracle:

```
crates/vesper-agent/src/planning/
├── mod.rs                  # declares the planning subtree
└── vesper_lens/
    ├── mod.rs              # VesperLens entrypoint (review_artifact)
    ├── types.rs            # LensFeedback / DomAnnotation / Action / LensError
    ├── injector.rs         # inject_review_overlay(html) -> String (owned overlay)
    ├── http.rs             # pure HTTP/1.1 request parser + response builders
    └── server.rs           # raw tokio::net::TcpListener loopback server
```

### Hard constraints (PRD §2)

1. **Zero new external dependencies.** The server is built on
   `tokio::net::TcpListener`, which is already a workspace dependency of
   `vesper-agent`. The per-crate Cargo entry adds only the `net` and
   `io-util` *features* on the existing `tokio` workspace pin — no version
   bump, no new crate, no `axum`/`actix-web`/`hyper`.
2. **Native Rust only.** No `std::process::Command` to shell out to Node,
   `npx`, or any external runtime. The overlay script is owned Rust-string
   source served verbatim — never executed on the Vesper side.
3. **Loopback-only, ephemeral ports.** The TCP listener binds to
   `127.0.0.1:0` and accepts the OS-assigned port. No wildcard bind, no
   public interface, no DNS-rebinding surface.
4. **Single connection, single turn.** The server accepts one connection,
   serves the injected HTML on `GET /`, accepts JSON feedback on
   `POST /feedback`, shuts the listener down, and returns. There is no
   session state, no auth token, no long-lived channel.
5. **Existing tests stay green.** The 954+ workspace tests are not modified.

### Data contract (PRD §3.D)

VesperLens defines its **own** minimal JSON contract — not ORACEL's
richer prompts/layout-warnings/artifact-failures model. The overlay posts:

```json
{
  "action": "approve" | "reject" | "modify",
  "annotations": [
    { "selector": "css-selector-or-dom-path",
      "comment": "human note",
      "suggested_html": "<optional>" }
  ],
  "notes": "free-form overall notes"
}
```

This is parsed into native `LensFeedback` / `DomAnnotation` / `Action` types
via `serde`. Wire format and Rust types are 1:1.

### Overlay script ownership

`injector.rs` ships a self-contained, ~150-line vanilla-JS overlay string
written for this crate. It does **not** import `chrome-client.js`,
`artifact-sdk.js`, or any other ORACEL module. It renders a small
floating review panel (Approve / Reject / Modify + optional annotations),
POSTs the contract above, and shows success. All display strings are
hard-coded literals under our control — no prompt text is templated from
incoming content, which closes the prompt-injection vector the reference
source's scanner flagged.

### Planner wiring scope

VesperLens is exposed as a library API:
`VesperLens::review_artifact(&self, html: &str) -> Result<LensFeedback,
LensError>`. Forcing every planner step through it would break existing VRO
behavior; instead it is **available** to be called from a planner step that
explicitly opts into human review (e.g., a future HTML-generation branch).
The VRO-11 milestone ships the oracle; the planner integration point is
documented here and can be added in a follow-up without an ADR amendment.

### VRO-11.2 — Planner seam and context injectionVRO-11.2 closes the planner-wiring gap without changing any existing VRO
control flow:

- New trait `LensReviewPort` in
  `crates/vesper-agent/src/vro/lens_integration.rs` abstracts the lens.
  The orchestrator stays pure; the composition boundary (TUI binary)
  supplies a concrete impl wrapping `VesperLens::review_artifact`.
- `VroOrchestrator` gains an optional `lens_port: Option<Arc<dyn
  LensReviewPort>>` field, a `with_lens_port(port)` builder, and an async
  `maybe_review_html_artifact(html, on_diagnostic)` method that returns
  `None` when no port is configured OR the input does not look like an
  HTML artifact (see `looks_like_html_artifact`).
- The host calls `maybe_review_html_artifact` when a tool output arrives.
  When a port is configured and the text is HTML, the port's `review`
  fires its own `on_url` callback; `maybe_review_html_artifact` adapts
  that into the PRD §4 diagnostic line
  (`[VesperLens] Artifact ready for review. Open: <URL>`) and forwards
  it to the host's `on_diagnostic` sink (TUI status line).
- After `review` returns, the host injects
  `feedback_as_context_message(&feedback)` into the conversation as a
  `role: Tool` message so the next model turn sees the human's verdict
  (APPROVED / REJECTED / NEEDS MODIFICATION) and any per-selector
  annotations (PRD §4: "context injection").

**Zero breakage.** When `lens_port` is `None` (the default), every
existing orchestrator method is byte-identical to VRO-10. All existing
workspace tests pass unchanged; the 16 new tests (13 lens_integration +
3 orchestrator-wiring) cover the seam in isolation.

### VRO-11.4 — Explicit tool replaces implicit interceptor (UX Overhaul)

The VRO-11.3 `LensObservingInvoker` was an implicit file-save interceptor:
every successful `write_file(.html)` call was silently intercepted and
routed through VesperLens review. Direct reconnaissance of the ORACEL
source code proved this is an architectural anti-pattern — ORACEL uses
**zero interception** and relies entirely on the model's judgment to
explicitly invoke `npx -y ORACEL <file>` when it wants human review.

VRO-11.4 course-corrects:

- **Implicit interceptor DELETED.** `LensObservingInvoker` and
  `html_artifact_for_write_file` are removed from `lens_integration.rs`.
  `VroOrchestrator::execute_react` no longer wraps the invoker — it passes
  it straight through, byte-identical to pre-VRO-11.3.
- **Explicit `request_human_review` tool.** A new tool is registered at the
  TUI composition boundary (`TuiToolService`). When the model wants human
  review of an HTML artifact, it calls
  `request_human_review(file_path="<path>")`. The tool reads the file,
  routes the content through `LensReviewPort::review`, blocks until the
  human submits (matching ORACEL's blocking poll semantics), and
  returns the verdict via `feedback_as_context_message`.
- **Trait signature update.** `LensReviewPort::review`'s `on_url`
  parameter is now tied to the `'a` lifetime of `&self` so concrete impls
  can call `on_url` from within the returned async block (needed because
  `VesperLens::review_artifact` calls `on_url` mid-async when the TCP
  listener binds).
- **Silent bypass fixed.** The TUI now ALWAYS constructs a
  `VesperLensPort` at startup and wires it into both `TuiToolService`
  (for the explicit tool) and `VroOrchestrator` (via `with_lens_port`).
  Before VRO-11.4, the orchestrator's `lens_port` was always `None`
  because no TUI code called `with_lens_port`.
- **Inline telemetry.** Tool execution logs are ripped out of the
  Reasoning sidebar and rendered DIRECTLY in the main Conversation panel
  via a new `TuiSession.live_trajectory` field. Both the direct path
  (`ToolStarted` / `ToolFinished`) and the ReAct trajectory stream
  (`drain_trajectory`) now route to `live_trajectory`, which
  `transcript_lines_for` appends after the transcript. This matches
  Codex / Claude Code / ORACEL host-agent rendering where the
  trajectory reads top-to-bottom naturally with the assistant's text.

This stays inside the ADR 0017 contract: the orchestrator never starts a
TCP listener directly; it always goes through the trait port. The change
is purely a shift in **trigger surface** — from implicit interception
(VRO-11.3) to explicit tool invocation (VRO-11.4), matching ORACEL's
proven architecture.

## Consequences

- **Positive:** VesperLens is fully native, zero-new-dep, loopback-only,
  testable without real network I/O (the HTTP parser is a pure function).
- **Positive:** Attribution is preserved. The ADR names ORACEL as the
  reference design under its MIT license; we copy no substantial code.
- **Positive:** The first runtime TCP listener in `vesper-agent` is narrowly
  scoped and cannot be reconfigured to bind off-loopback.
- **Negative:** New runtime behavior lives in `vesper-agent` rather than a
  composition app. This is acceptable because (a) it is loopback-only,
  (b) the planner calls it explicitly rather than the agent loop binding on
  startup, and (c) the architecture gate now covers this subtree.
- **Risk:** An HTML artifact the model generates could itself contain
  prompt-injection. The overlay trusts the served HTML the same way any
  browser does; the *agent* receives only the JSON contract (not raw HTML),
  so injection-via-feedback is bounded to attacker-controlled `comment` /
  `notes` strings, which are routed through the same untrusted-input
  discipline as any other user-provided text.

## Verification

- `cargo xtask architecture` — must continue to pass (no forbidden terms,
  no new external crate dep, allowlist unchanged for `vesper-agent`).
- `cargo test -p vesper-agent --lib planning::vesper_lens` — pure-function
  tests for the HTTP parser (no network in the parser tests).
- `cargo test -p vesper-agent --lib planning::vesper_lens` — loopback
  integration tests using `127.0.0.1:0` (deterministic, fast, run in CI).
- `cargo xtask verify` — full workspace gate stays green (954+ tests).

## References

- PRD: VRO-11 (VesperLens Native Oracle Port), §1–§4.
- Reference design: `kunchenguid/ORACEL` (MIT, Copyright (c) 2026 Kun
  Chen). Cloned to `/home/alex/Projects/ORACEL` under explicit user
  authorization. Read for architectural pattern only; no code copied.
