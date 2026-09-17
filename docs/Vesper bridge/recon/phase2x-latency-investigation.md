# Phase 3 — Latency Investigation & Suite Speedup (Session 2x)

**Date:** 2026-09-16 · **Status: PASS — reported latency root-caused to test-loop cost, not the product; product path measured at 22 ms end-to-end**

## Alex's report

"Almost 1 min to open Elisa, 20s to press play."

## Measurement-first response (no guessing)

| Layer | Measured | Verdict |
|---|---|---|
| DBus `Play` | **0.00s** (×3) | fine |
| DBus `get-property` | **0.00s** (×3) | fine |
| `busctl --user list` (discovery) | **0.01s** (×3) | fine |
| Elisa cold start → MPRIS-ready | **0.67–1.11s** (×3) | fine |
| Live adapter test (cached build) | **0.19s** | fine |
| **Full harness lib suite** | **10.0s → 1.4s** | **the bug** |

## Root cause

The product was never slow. The perceived latency was **my verification
procedure**: two enrollment-ceiling tests each carried a **10-second
override window** whose stalling reviewer sleeps out the full window
(their purpose — pinning that the ceiling bounds a 600s stall — is sound),
and I ran the **entire suite** between every interactive step. Alex's
"20 seconds to press play" was 10s of ceiling tests + recompile + suite
overhead, not Bridge.

## Fixes

1. **Test override 10s → 1s** (`enrollment_wall_clock_ceiling_bounds_the_total_window`,
   `enrollment_ceiling_covers_the_contract_ladder_too`): the ceiling
   mechanism is identical at any window size; a 1s window pins it just as
   truthfully. Elapsed assertions tightened 30s/60s → 10s so the tests can
   never silently regress to slow. **Acceptance group: 10.2s → 1.15s;
   full lib suite: 10.0s → 1.26s.**
2. **Procedure**: targeted filters (`live_mpris`, `hardening`, ~0.2s) for
   iteration; full suite only at phase boundaries. (Already the practice
   after this incident; now documented.)
3. **Regression guard**: new live `live_product_path_latency_budget` pins
   the interactive path (discovery → connect → observe → execute) under
   **1s** — measured today at **22 ms** (discovery 16, connect 3, observe 0,
   execute 3). If Bridge ever becomes slow, this test fails, not the user's
   patience.

## Honest scope note

Elisa's own cold start (0.7–1.1s) is the application's, not Bridge's —
Bridge does not and must not pre-launch applications (§1). The "1 min to
open" figure also included full-suite runs I executed between actions; a
single `elisa` launch plus MPRIS readiness is ~1s.

## Receipts (fresh)

| Gate | Result |
|---|---|
| harness `--features bridge --lib` | **149/0 in 1.24s** (was 10.0s) |
| harness feature-off | **122/0 in 1.37s** |
| `vesper-bridge` (5 suites) | 48/0 |
| clippy stable **and** 1.88.0 | 0 errors |
| fmt / architecture / acceptance | clean / 28 pkgs / 23/23 |
| **LIVE** product-path latency | **22 ms** (< 1s budget, test-enforced) |
| **LIVE** MPRIS control | PASS |

## Files

- `crates/vesper-harness/src/acceptance_tests.rs` — override 10s→1s,
  elapsed bounds 30/60s→10s
- `crates/vesper-harness/src/bridge_latency_tests.rs` — new live latency
  budget test (product-path < 1s)

## Verdict

**PASS.** The latency complaint was real and is resolved: not by speeding
up Bridge (it was already 22 ms) but by fixing the 10s test-window tax and
adding a latency budget test so the interactive path can never silently
degrade.
