# ADR 0030: Vesper Bridge Application-Control Subsystem

Status: ACCEPTED (promoted from VB-PRD-001 phase0 recon drafts AD-01…AD-08,
which this ADR supersedes as the durable record; the recon file remains
historical evidence)

## Context

VB-PRD-001 requires provider-neutral application control (desktop/video/
browser applications) without replacing the harness or weakening safety.
Phase 0 recon compared vendor-native connectivity, documented scripting,
a community MCP adapter, and generic visual control, and recorded eight
draft decisions. Phases 1–2 then landed the described architecture and
their contracts were hardened by 13 audit-found defects (increments
2d–2n), making the drafts the implemented reality rather than proposals.

## Decision

1. **Bridge core is pure and provider-neutral** (`vesper-bridge`:
   identity, capability manifest, operation envelopes, session/lease/
   fence state, journal port; no clock, network, or capture). Application
   and OS behavior composes in `vesper-harness` behind the default-off
   `bridge` feature; both hosts consume one service and one shared
   `/bridge` command implementation (BR-21).
2. **MCP is one transport option, not the model.** Application sessions,
   resource identity, authority generations, leases, and verification
   are transport-agnostic; a transport connection is not an application
   session. The plugin loader stays declarative-only; executable
   sidecars are separately approved, digest-pinned leaf adapters (AD-01).
3. **Route ladder** (AD-02): documented application API → semantic
   (DOM/AT-SPI/UIA) → bounded visual input. Fallback is a fresh decision
   re-entering the same gates; a denial or missing capability never falls
   through to a broader route.
4. **Host-attached by default, disclosed** (AD-03): an existing
   application keeps its ambient authority; Bridge never calls argument
   validation a sandbox. Strong isolation required and unavailable ⇒
   refuse.
5. **Owned sessions, per-resource exclusive leases with fencing**
   (AD-04); leases are exclusive per resource across sessions.
   Emergency input release survives ordinary revocation (§8.4) and is
   settled only by an explicit host confirmation after driver
   acknowledgement — never by an ack string alone.
6. **Stop is independent of model inference** (AD-05, NF-02): production
   stop/resume/settlement close and reopen admission synchronously from
   both hosts; outcomes are classified (dispatched/applied/verified/
   partial/failed/cancelled/unknown) by evidence. Every classification
   state is reachable through a core API and terminal states are not
   success unless independently verified.
7. **Reuse existing seams** (AD-06): tool registration, image parts
   (`ToolResult::with_media`, ≤8), permission gates, cancellation — no
   second agent loop or provider path.
8. **MCP dual-era client** (AD-07): legacy initialize stays default;
   the 2026-07-28 negotiated path lands behind tested negotiation only.
9. **Non-destructive Resolve adapter over documented scripting** (AD-08):
   no project-database writes or undocumented XML mutation; intent
   journal + reconcile-before-retry; render-queue submission is never
   "done" — verification is ffprobe/readback/decode evidence.
10. **Core-bounded state** (NF-05/07/08, AT-37): session records,
    outstanding jobs, and the production journal are bounded by the core
    itself with newest-window eviction; request ids are monotonic and
    survive eviction.

## Alternatives considered

- Python/Node orchestrator beside the harness: rejected (AD-01) —
  bypasses the permission gate and provider contracts.
- Executable plugin payloads: rejected — the loader is declarative-only;
  execution smuggled through profiles/workflows is prohibited.
- Driver ack strings as postconditions: rejected (AD-05/08) — an ack is
  not a postcondition; verification requires independent evidence.
- "Bounded by caller" state: rejected — hidden coupling; the core owns
  its budgets.

## Consequences

- Bridge ships default-off; enabling it changes the tool surface by
  exactly eight model tools plus host `/bridge` commands, and the
  disabled path is byte-identical (AT-01, kernel-accounted in both
  hosts).
- Quarantine is resolvable only by evidence-backed reconciliation
  (Quarantined → Recovering), never by resume; pause cannot weaken it.
- Disconnect surfaces unresolved input leases and outstanding jobs
  rather than discarding them.
- Live-application lanes (Resolve, generic driver) remain BLOCKED until
  their installations exist; no adapter may be advertised before
  authentication, catalog, transport, fixture, and CI evidence.

## Evidence

- Phase evidence: `docs/Vesper bridge/recon/` (phase0 report; phase1–2
  and increment reports 2d–2n including the final audit and its
  test-edited-to-green correction).
- Contract verification: `cargo test -p vesper-bridge` (68 tests),
  harness bridge suites (142/142), both hosts' feature-on/off suites,
  full workspace `cargo test --workspace` (162 targets, 0 failures),
  `cargo xtask acceptance` 23/23, MSRV 1.88 clippy clean.
- AT-01 kernel-accounted observation: ACP integration test + TUI PTY
  script.
