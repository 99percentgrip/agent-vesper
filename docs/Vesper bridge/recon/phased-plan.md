# Vesper Bridge — Phased change plan and ADR drafts (VB-PRD-001 §3.5)

> **ADR status (2026-09-15):** AD-01…AD-08 below were promoted to the
> accepted **ADR 0030** (`docs/adr/0030-vesper-bridge-application-control.md`)
> once Phases 1–2 landed the described architecture and the final audit
> verified its contracts. This file remains as the historical drafting
> evidence; the ADR is the durable decision record.

## ADR drafts (accept before Phase 1 code lands)

### AD-01 Native Bridge service, data-only profiles
`vesper-bridge` is a first-party crate with pure contracts; application
behavior composes in `vesper-harness`. The existing plugin loader stays
declarative-only; executable sidecars are separately installed, digest-pinned
artifacts with their own provenance record, never plugin payloads.

### AD-02 Hybrid route ladder
Route preference: documented native application API → semantic
(DOM/AT-SPI/UIA) → bounded visual input. Fallback is a fresh execution
decision re-entering the same plan/permission/verification gates; a denial
or missing capability never falls through to a broader route.

### AD-03 Host-attached vs isolated execution
Default is host-attached with explicit disclosure of the target's ambient
authority. When a task requires real isolation and the environment cannot
provide it, Bridge fails closed. No silent downgrade; no calling argument
validation a sandbox.

### AD-04 Owned sessions and exclusive mutation leases
One owner per application session; resource-scoped mutation leases with
fencing generation and expiry; a desktop/seat-wide foreground-input lease.
The supervised worker rejects stale generations before dispatch. Routes
that cannot enforce fencing settle through quarantine, not blind retry.

### AD-05 Independent stop and unknown-outcome settlement
Admission closure is reachable without a model turn (both hosts plus a
watchdog). Emergency input release survives lease revocation. Outcomes are
classified (dispatched/applied/verified/partial/failed/cancelled/unknown)
by evidence, never by driver ack strings.

### AD-06 Reuse of existing seams
Tool registration via `ToolService`/`with_service`; images through
`ToolResult::with_media` (≤8 image parts) and the provider capability gate;
permissions through `check_tool_permission` and the approval ports;
cancellation through the existing `CancellationSignal`. No second agent
loop, no new provider path.

### AD-07 MCP dual-era client (2025-06-18 + 2026-07-28)
Keep the current legacy initialize flow as the default for existing
servers. Add a negotiated modern path (`server/discover` +
per-request `_meta` protocol metadata) behind explicit feature/tests,
pinning both sides of each supported pair. Do not adopt an external Rust
MCP SDK without its own MSRV/license evaluation; the in-tree client stays.

### AD-08 Non-destructive Resolve adapter over documented scripting
First-party Rust sidecar speaking the Studio external scripting API. No
project-database SQL, no undocumented XML mutation. Intent journal +
reconcile-before-retry (the documented silent-failure catalogue makes
return-value-only success untrustworthy). Verification = ffprobe + timeline
readback + bounded decode; render-queue submission is never "done".

## Phased plan

### Phase 1 — `vesper-bridge` pure contracts + fake driver
- New crate `crates/vesper-bridge`: deps `vesper-domain`,
  `vesper-security` only (matches `allowed_dependencies` philosophy; entry
  added to `xtask/src/main.rs::allowed_dependencies` in the same change).
  Add `crates/vesper-bridge/AGENTS.md`; update `crates/AGENTS.md` child index.
- Contents: identity model (generation-bound application binding), versioned
  capability manifest (availability × implementation status), operation
  envelopes + typed results, session/operation state machines, leases with
  fencing, deadlines, error contract, minimal journal port.
- Fake driver lives in `vesper-testkit` (test-only by contract); visibly
  named `Fake*`, cannot be constructed in release paths.
- Tests: denial, stale identity, duplicate requests, unknown outcomes,
  shutdown, lease exclusion (U class). Gates: `cargo xtask architecture`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace --all-features`, MSRV check.

### Phase 2 — host integration (both hosts)
- `vesper-harness`: `bridge_service.rs` behind default-off `bridge` feature
  (mirrors `swarm`/`web` precedents); `BridgeToolService` registered via
  `with_service`; stop/admission wiring to existing cancellation ports;
  progress to TUI + ACP; `/bridge` slash commands through the shared catalog
  (vesper-domain), host-neutral.
- AT-01 disabled-path proof: byte-identical registry/behavior without the
  feature and with it off.
- Long-lived connections: only the harness-owned stdio child; the scoped
  `tools()` path remains for one-shot discovery.

### Phase 3 — Resolve vertical slice (BLOCKED until install)
- Sidecar (Rust, system Python interop per scripting API), capability
  manifest distilled from the documented API, non-destructive workflow:
  inspect/import → working project → timeline + supported cuts/title/audio
  → draft render → ffprobe + readback verification.
- Fixed media fixture under `fixtures/bridge/` (LF-pinned via
  `.gitattributes` if any JSON lands there).

### Phase 4 — generic driver lane
- Cua Driver v0.28.1 as supervised stdio child (digest from release
  `checksums.txt`), `bounded`-mode-equivalent manifest, X11/XWayland lane
  certification on this machine; KWin-native surfaces refuse.
- Second application workflow through the same core (AT-36): prefer the
  existing browser route where semantic controls suffice.

### Phase 5–6 — hardening/generality, then experimental games
- Support manifests, upgrade invalidation, recovery/retention audits,
  budget telemetry, reviewed workflow learning; then ViZDoom-class
  controlled environment (own flags), never weakening local safety.

## Explicit blockers (from Phase 0)

1. Harness enrollment paragraph ceiling — must be repaired before native
   acceptance can freeze this PRD (see `PRD-ENROLLMENT-NOTE.md`).
2. No Resolve installation on this machine — Phase 3 lane BLOCKED.
3. Cua install authorization + XWayland lane certification — Phase 4 gate.
4. Portal ScreenCast v5 metadata availability probe — before capture work.
