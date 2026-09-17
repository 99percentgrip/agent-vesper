# Vesper Bridge — Threat model and test plan (VB-PRD-001 §3.4)

Owner column names the subsystem that must make the property true;
"phase" names the PRD phase where the test first becomes executable.
Evidence classes: U = deterministic unit (fake driver), I = integration
(real process paths, both hosts), N = native/live application, P = performance.

## Trust boundaries

1. Model ↔ host policy. The model proposes; `vesper-policy`/host consent
   disposes. Skills and model text never carry authority.
2. Host ↔ Bridge core. The core is pure; the host injects identity, leases,
   approvals, and the clock.
3. Bridge ↔ driver sidecar. stdio, sanitized env, pinned executable
   identity, bounded responses; no ambient PATH resolution.
4. Driver ↔ OS (portals, compositor, accessibility). Consent surfaces
   belong to the OS/user; Bridge must surface, never bypass, them.
5. Bridge ↔ target application. The application has ambient authority of
   the logged-in user; Bridge's argument validation is NOT a sandbox.
6. Bridge ↔ provider. Observation images leave the machine only under an
   explicit image-egress grant (separate from tool permission).
7. Bridge ↔ durable storage. Screenshots/recording retention is opt-in and
   audited (NF-11).

## Risk register

| # | Risk | Boundary | Owner | Mitigation | Tests |
|---|---|---|---|---|---|
| R1 | Prompt injection via subtitle/filename/AX/screenshot text | 1 | bridge observation contract | delimit untrusted content; no authority in observations; separate fields | AT-09 |
| R2 | Ambient desktop authority mistaken for sandboxing | 5 | approval UX + docs | host-attached disclosure per approval; isolation decision explicit | AT-39 |
| R3 | Focus/geometry race (input to wrong window) | 4 | binding + freshness | generation-bound binding, fresh observation precondition, refusal on drift | AT-03, AT-13 |
| R4 | Timeout-after-commit, duplicate creates | 3↔app | journal + reconcile | intent record pre-dispatch, task-linked names, reconcile before retry, no auto retry of non-idempotent ops | AT-18, AT-19, AT-20 |
| R5 | Driver success string treated as postcondition | 5 | verifier port | independent verification (readback/ffprobe/decode) for every applied claim | AT-15, AT-27, AT-28 |
| R6 | Bypass via raw MCP gateway or worker registry | 1↔2 | registry composition | Bridge ops not reachable ungated; compound tools classified per action; prefix audit | AT-10 |
| R7 | Held input leaks after stop/crash | 3 | watchdog | admission closure + emergency release path surviving lease revocation | AT-21, AT-22, NF-02, NF-03 |
| R8 | Cancellation ≠ settlement (render keeps running) | 5 | job model | outstanding-job reporting until app confirms; no false rollback | AT-23 |
| R9 | Screenshot/metadata egress without consent | 6 | egress grants | per-destination grants; redaction before transmission; clipboard off | AT-31 |
| R10 | Stale approval reused after app/adapter/schema change | 1 | authority generation | approval binds material facts; generation bump invalidates | AT-08, AT-30, AT-35 |
| R11 | Unbounded memory/process under slow consumers | 2 | bounded queues | NF-05/07/08 budgets; supersede-not-drop for observations | AT-37 |
| R12 | Naming-guard/architecture drift | repo | xtask | add crate to allowlist via ADR; no exemption broadening | CI gates |
| R13 | Resolve silent failures (documented) | 5 | Resolve adapter | treat False/None as unknown unless verified; unique timeline names; never rely on return values alone | AT-15, AT-19 |
| R14 | Quarantine bypass on restore/replay | 2 | session policy | restored history is inspectable only; no executable replay | AT-40 |

## BR ownership map

| BR | Owner subsystem | Primary scenario tests |
|---|---|---|
| BR-01 Discovery | bridge core + harness service | AT-02, AT-04 |
| BR-02 Exact attachment | bridge identity model | AT-02, AT-03 |
| BR-03 Capability negotiation | bridge manifests | AT-04, AT-05, AT-32 |
| BR-04 Connection lifecycle | harness supervision | AT-24, AT-37, AT-44 |
| BR-05 Skills separation | agent skill orchestration (existing) | AT-09, AT-42 |
| BR-06 Planning | agent planning (existing) | AT-41 |
| BR-07 Mode enforcement | existing permission gate + tool classes | AT-06 |
| BR-08 Authorization | policy + host approval ports | AT-07, AT-08, AT-10, AT-39 |
| BR-09 Observation | bridge observation contract | AT-11, AT-12, AT-13, AT-14 |
| BR-10 Model compatibility | existing provider capability gate | AT-11 |
| BR-11 Action execution | bridge typed operations | AT-05, AT-15 |
| BR-12 Verification | bridge verifier port | AT-14, AT-15, AT-27, AT-28 |
| BR-13 Long-running jobs | bridge job model | AT-23 |
| BR-14 Concurrency | leases | AT-16, AT-17 |
| BR-15 Stale-state protection | preconditions | AT-03, AT-13, AT-29 |
| BR-16 Duplicate protection | journal + dedup | AT-18, AT-19, AT-20 |
| BR-17 Stop and takeover | stop path (both hosts) | AT-21, AT-22, AT-23 |
| BR-18 Recovery | quarantine rules | AT-19, AT-20, AT-24, AT-25, AT-37, AT-40 |
| BR-19 Non-destructive media | Resolve adapter policy | AT-25, AT-26, AT-29 |
| BR-20 Export delivery | verifier | AT-27, AT-28 |
| BR-21 Host parity | both host compositions | AT-34 |
| BR-22 Privacy | egress grants | AT-09, AT-31, AT-39 |
| BR-23 Adapter integrity | signed profiles + pinned sidecars | AT-30 |
| BR-24 Auditability | observability (existing) | AT-31, AT-40 |
| BR-25 Diagnostics | error contract | AT-04, AT-29, AT-32 |
| BR-26 Workflow learning | existing learning + version binding | AT-35, AT-42 |
| BR-27 Generality | core purity | AT-36 |
| BR-28 Extension safety | manifest generations | AT-30, AT-33, AT-35 |
| BR-29 Accessibility | host UX | AT-34, AT-43 |
| BR-30 Feature isolation | composition | AT-01 |

## AT test-class assignment

- Phase 1 (fake driver, unit): AT-02, AT-03, AT-05, AT-08, AT-12, AT-13,
  AT-14, AT-16, AT-18, AT-20, AT-40, AT-41 — U.
- Phase 2 (host integration): AT-01, AT-06, AT-07, AT-09, AT-10, AT-11,
  AT-21, AT-22, AT-24, AT-34, AT-37, AT-43 — I (both hosts), plus AT-33 with
  peer fixtures.
- Phase 3 (Resolve slice): AT-15, AT-17 (Resolve half), AT-19, AT-23, AT-25,
  AT-26, AT-27, AT-28, AT-29 — N; BLOCKED until Resolve installed.
- Phase 4 (generic driver): AT-17 (input half), AT-32, AT-36 — N on the
  certified lane; AT-38 deferred.
- Phase 5 (hardening): AT-30, AT-31, AT-35, AT-42, plus P evidence for
  NF-02/03/04 (and NF budgets).

NF budgets are tested at the phase that owns the mechanism; the measurement
protocol (p50/p95, n, warm/cold, versions) is defined in the PRD §14 and
recorded per lane in the support manifest.

## Current status (Phase 0)

All 44 AT rows: **NOT TESTED**. Resolve-dependent rows are additionally
**BLOCKED** pending application install. The KWin refusal path (AT-32 part)
is designed to *refuse* — a refusal there is a pass, provided it is the
designed structured refusal and not a crash.
