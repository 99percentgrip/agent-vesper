# Acceptance enrollment ceiling repair (VB-PRD-001 intake)

## Objective

Unblock native PRD enrollment for VB-PRD-001 (Vesper Bridge PRD rev 1.0),
which the enrollment reader refused with `PRD requires 1–256 source
paragraphs`. The PRD's own scope is unchanged; the blocker was a harness
bounds constant smaller than real specifications.

## Methods and commands

- Reproduced the enrollment reader's paragraph split exactly
  (`\n\n`-separated, empty-filtered) over the PRD plus every `AGENTS.md`
  along the workspace route: PRD 234 + docs 16 + root 34 = **284**.
- Red-first regression: `cargo test -p vesper-harness --lib
  enrollment_bounds` — both new tests failed against the old constant
  (`Err("PRD requires 1–256 source paragraphs")`), then passed after the
  fix. The real-PRD probe is `#[ignore]`d and passes with
  `cargo test -- --ignored real_vesper_bridge`.
- Gates after the change: `cargo clippy -p vesper-domain -p vesper-harness
  --all-targets --all-features -- -D warnings` (clean), `cargo fmt
  -- --check` (clean), `cargo test -p vesper-harness --lib` (116 passed,
  2 ignored), `cargo xtask architecture` (27 packages),
  `cargo xtask naming-guard` (clean, baseline regenerated in the same
  change for the new pinned upstream docs), `cargo xtask acceptance`
  (**23/23 exact cases**, 8,392 ms).

## Files changed

- `crates/vesper-domain/src/acceptance.rs` — `MAX_REQUIREMENTS` 256 → 512
  with a comment recording the real measured case.
- `crates/vesper-harness/src/acceptance.rs` — honest error text
  `PRD requires 1–{MAX_REQUIREMENTS} source paragraphs (found N); the
  input stays byte-capped at 256 KiB`; registers the new test module.
- `crates/vesper-harness/src/acceptance_enrollment_bounds_tests.rs` —
  red-first bounds tests + ignored real-PRD probe.
- `xtask/naming-guard-baseline.json` — regenerated (15 new frozen hits are
  the pinned upstream recon documents, which quote third-party product
  names verbatim; no new hits in Vesper-authored prose).
- Docs: `docs/Vesper bridge/PRD-ENROLLMENT-NOTE.md` updated with the
  repair and the still-old-binary caveat.

## Exact evidence

- Pre-fix failure (red):
  `enrollment_accepts_prd_above_the_old_paragraph_ceiling` panicked with
  `Err("PRD requires 1–256 source paragraphs")`; post-fix: `test result:
  ok. 2 passed; 0 failed`.
- Real-PRD probe passes:
  `real_vesper_bridge_prd_opens_under_the_raised_ceiling` (`ok. 1 passed`).
- `cargo xtask acceptance`: `Acceptance regression gate: 23 exact cases
  passed in 8392 ms.`
- The session's `acceptance_enroll` tool still refuses with the **old**
  message because its hosting process (`/home/Alex/.local/share/
  agent-vesper/agent-vesper-tui`, PID 151417, binary built 2026-09-15
  02:54) predates the repair — verified by byte-scanning the installed
  binary: it contains `1–256 source paragraphs`; the workspace
  `target/debug/agent-vesper-acp` contains the corrected
  `(found N)` form instead. Per the standing rule, commit/push work does
  not authorize replacing Alex's local installation, and the running TUI
  was not touched.

## Deviations

- No scope substitution: the original 684-line PRD remains the only
  enrolled-scope candidate; nothing reduced was enrolled.
- The session-level gate therefore still reports `PRD: Missing` — a
  hosting-process restart/refresh is required before this session's
  `acceptance_enroll` can freeze the contract. That restart is left to
  Alex (his TUI is mid-session on another task surface).

## Unresolved items

1. Restart the hosting agent process on the repaired binary, then rerun
   `acceptance_enroll docs/Vesper bridge/Vesper_Bridge_PRD.md`.
2. The 284-paragraph enrollment will then require the contract ladder to
   map every source paragraph; enrollment-visibility ceiling (~17 min
   worst case per the 2026-09-14 audit) applies.
3. Everything else in the Phase 0 verdict stands: Resolve lane BLOCKED
   (no install), XWayland lane pending Cua authorization, all 44 AT rows
   NOT TESTED.

## Readiness effect

The mechanical blocker for enrolling real, larger PRDs is fixed in source
with regression coverage and all repository gates green. Native acceptance
of VB-PRD-001 itself remains **not completed** until the hosting process
runs the repaired code and the enrolled contract is verified — no
completion is claimed here.
