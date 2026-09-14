# Acceptance enrollment visibility & bounds execution

Date: 2026-09-14. Status: **complete**.
**Audit 2026-09-15**: AC-3 was untested and the ceiling missed the
contract ladder — repaired red-first; see the PRD audit note and
`acceptance-enrollment-audit.md`.
Owner PRD: [Acceptance enrollment visibility](../acceptance-enrollment-visibility-prd.md).
Trigger: Alex's field report — with acceptance enabled, the agent
"freezes and does nothing"; the slash command itself responded fine.

## Root cause (verified on the tree, not assumed)

- `acceptance_enroll` runs nested reviewer agents (scope review → up to
  3 contract+coverage attempts) whose only progress port was an internal
  `read_file` counter (`acceptance.rs:273-286` pre-fix). **Zero events
  reached the host UI**: one `ToolStarted` blip, then silence for many
  minutes.
- The contract ladder retried up to 3 times; total enrollment time was
  unbounded (worst case ~7 sequential nested provider runs).
- Failed enrollments returned tool errors that the model would retry, or
  grind toward the iteration ceiling under completion-refusal messages —
  silent the whole way.

## Changes

| File | Change |
|---|---|
| `crates/vesper-agent/src/agent_loop.rs` | + `AgentProgressEvent::Status { text }` (bounded harness-internal stage line; not model content) |
| `crates/vesper-harness/src/lib.rs` | `WorkerFactory::with_progress` / `progress()` — optional host progress sink carried into harness-internal reviewers |
| `crates/vesper-harness/src/acceptance.rs` | `NativeAcceptanceReviewer.progress` + `stage()` lines per review stage; `ForwardingProgress` bridges nested reviewer tool activity as `Status`; contract ladder 3→**2** attempts; loud bounded failure with "stop retrying, ask the user" guidance; enrollment wall-clock ceiling **300 s** (`tokio::time::timeout` around the scope review); refusal emits a host `Status` line, saves nothing, stays unenrolled; `AcceptanceSession::open_with_progress` |
| `apps/agent-vesper-tui/src/main.rs` | session-lifetime `SessionStatusPort` + `acceptance_progress`/`acceptance_status_rx`; factory wired with the port in `/acceptance`; status lines drained every frame into the activity feed (`◌ <line>`) |
| `apps/agent-vesper-acp/src/lib.rs` | `Status` rendered as a reasoning-delta line; enrollment factory carries the turn's progress port (same event surface as every other tool) |
| `crates/vesper-harness/src/acceptance_tests.rs` | 3 new tests (visibility, ladder cap, refusal loudness/no-save) |

## Red-first receipts

- **Compile-level**: with only the production fix reverted, the new tests
  cannot compile (`no method with_progress` / `no function open_with_progress`)
  — pre-fix code had no channel at all.
- **Runtime-level**: with all progress emission stripped from the fixed
  tree (exact pre-fix silence), the visibility test fails verbatim:
  `enrollment scope-review Status line missing: []`. With the fix: green.
- Ladder-cap test: a refusing contract reviewer is attempted **exactly
  twice** then fails loudly (`bounded review attempts` + `ask the user`).
- Refusal test: refusal emits the Status line, `Do not retry` guidance,
  saves no enabled settings, stays unenrolled.

## Fix iteration honesty

- First `apply_patch` on the reviewer bridge missed (context drift); exact
  context re-read, then applied.
- The `CompactionFailed` arm was briefly mis-edited in the TUI; caught and
  restored before commit.
- Test 1 initially asserted via the session sink (`reviewer_progress`);
  stage lines actually flow through the reviewer's factory port — test
  rewritten against `NativeAcceptanceReviewer::new(factory.with_progress)`
  with per-turn fake-provider scripts (three iterations to get the
  fixture protocol right: read_file turn satisfies the inspection guard,
  verdict JSON is its own turn).
- A clippy `match`→`if let` lint fixed; fmt clean.

## Verification receipts

- Enrollment/visibility tests: **4/4 green** (1 rewritten + 2 new + 1
  pre-existing).
- `cargo test -p vesper-harness`: 112 passed / 0 failed.
- Workspace all-features: **2,329 passed / 0 failed** (2,326 + 3 new).
- `cargo xtask acceptance`: **23/23** — the completion-assurance gate
  (ADR 0028) is untouched and passing.
- clippy `-D warnings`: clean · fmt: clean · naming-guard: 18 frozen.

## Scope guarantee (G4)

No verdict, receipt, review-prompt, or evidence rule changed: reviews
still gate; refusals still refuse; nothing auto-completes; settings are
still never saved on refusal. The change is strictly **visibility +
bounds + loud failure**.

## Unresolved items

- Real acceptance of the *new* UX on Alex's machine (update → enable →
  run; status lines should appear within seconds of the model's
  `acceptance_enroll` call, and the worst case is a ≤5-minute bounded,
  visible failure instead of an unbounded silent hang).
- The ACP client-side rendering of `Status` inherits the existing
  reasoning-delta styling; no dedicated ACP event type was added.

## Readiness effect

Enabling acceptance can no longer look like a freeze: enrollment reviews
stream stage lines to the host, are capped at ~5 minutes total, and any
failure ends in one actionable message telling the model to ask the user
— instead of an unbounded silent grind.
