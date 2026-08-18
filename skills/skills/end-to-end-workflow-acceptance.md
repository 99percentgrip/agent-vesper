---
name: end-to-end-workflow-acceptance
description: The owner-mandated completion standard for any user-facing interactive feature, born from the VesperLens autopsy (v0.20.29–v0.20.41 shipped components that never worked end-to-end). Done means a user completes the whole workflow in the installed release — not compile + unit tests + CI green. Apply BEFORE claiming any browser-, TUI-, or tool-loop feature complete.
version: 1.2.0
author: Agent Vesper library (migrated from legacy GLM-ACP)
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [feature-delivery, integration-testing, release, vesperlens]
prerequisites:
  commands: [cargo, gh]
---

# End To End Workflow Acceptance

Apply to every user-facing/interactive feature (browser, TUI, agent tool
loop) before claiming it complete. Full audit:
`docs/foundation/vesperlens-end-to-end-acceptance-postmortem.md` in the
Agent Vesper repo.

1. FIX THE ACCEPTANCE LINE FIRST. Done = a user completes the workflow in
   the installed release using the actual TUI and browser: model invokes
   the tool -> host advertises and waits -> user interacts -> feedback
   returns to the SAME agent turn -> model can revise. Compile, unit
   tests, `cargo xtask verify`, and CI green are ENTRY criteria, never
   completion evidence.
2. PRODUCTION CALLER BEFORE COMPONENTS. Identify the exact
   composition/dispatch path that invokes the feature before building
   helpers. A port/trait whose production default is None with no override
   is OFF, not wired. Grep for non-test callers; zero callers means stop
   and wire the real host.
3. EXPLICIT MODEL-FACING TOOLS ONLY. Trigger via advertised tools (the
   request_human_input / request_human_review pattern). Never intercept
   inside one execution route (a ReAct-only interceptor is bypassed by
   every other strategy). Separate planning interviews (typed schema:
   radio/multi/free-text, optional, help text, recommended answers,
   free-form other, stable question IDs) from artifact review; both block
   and return feedback as tool output in the same turn.
4. ISOLATE UNTRUSTED ARTIFACTS. Artifact HTML renders in a sandboxed
   iframe WITHOUT allow-same-origin. Verdict buttons (Approve / Send
   changes / Reject / End session) live only in trusted outer chrome.
   Guard the feedback route with a per-session UUID token plus exact
   Host/Origin, content-type, and custom-header checks; confine to
   canonical workspace paths with extension and size caps.
5. LIFECYCLE OVER VISUALS. Sessions keyed by canonical file path
   (reopening reuses the URL); feedback queued independent of the waiting
   tool call (a cancelled wait must not lose it); serve relative assets;
   persist reload state and drafts (sessionStorage).
6. REAL-BROWSER GATE. Tests must press real buttons: controls visible and
   enabled, submit works, answers reach the agent, relative CSS/JS loads,
   zero console/network errors. Wire the scripts into `cargo xtask verify`
   and CI; until automated, report them as manual-only — never call them a
   gate.
7. LIVE-TEST THE INTERACTION MODEL for any UI change: wheel scrolling, the
   review URL appears exactly once and is actually openable (click +
   Ctrl+O), annotation mode never blocks normal artifact use, TODO visible
   without polluting chat history, single-column conversation plus a
   dedicated TODO sidebar.
8. INSTALLED-RELEASE PROOF. Reinstall via
   `AGENT_VESPER_VERSION=x.y.z sh scripts/install.sh`, spawn a FRESH
   browser (stderr fd inheritance is per-spawn; stale browsers keep
   spraying the TUI), run the full loop, and only then declare complete.
9. SHIP HONEST SCOPE. Advertise tools only when a real executor is
   configured; publish the not-implemented list with the feature; never
   claim parity with the Oracle reference project beyond evidence.

PITFALLS (VesperLens autopsy): an orchestrator seam that compiled, passed
unit tests, and was still dead because no production code called it; a
write_file interceptor covering one route only; a 180-second zero-tool turn
means the prompt failed to MANDATE execution (plan-then-stop is forbidden);
artifact JS killing the overlay (CSP meta, body wipe) was a symptom of
same-document design — isolation is the fix, not stripping symptoms;
harness gates (like skill learning) are SERVANT mechanics — satisfy them,
never let them defer the master's order.

## Provenance

Learned in the legacy GLM-ACP agent on 2026-08-17; migrated to the Agent
Vesper library.
