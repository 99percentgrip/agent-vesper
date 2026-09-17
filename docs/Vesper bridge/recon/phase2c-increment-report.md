# Vesper Bridge — Increment 3: provenance pinning, lane manifest, invariant coverage

## Objective

Advance everything that does **not** require the two blocked lanes
(Resolve install; Cua install authorization): pin driver provenance so
Phase 4 can start the moment authorization lands, publish the living
compatibility manifest with precise manual actions, and extend
deterministic contract coverage for invariants the earlier suites left
implicit.

## Methods and commands

- Fetched and pinned the upstream `checksums.txt` for
  `cua-driver-rs-v0.28.1`; recorded the release's **prerelease: true**
  flag (a fact that materially affects adoption: candidate-only until a
  stable promotion, which will require re-pinning).
- `recon/cua-driver-provenance.md`: asset digest
  (`a068b6e4…` linux-x86_64), size, publisher, lane decision (X11/
  XWayland only; KWin-native refuses by design), required run-time
  permissions, and the exact authorization blockers.
- `recon/compatibility-manifest.md`: living manifest — host environment,
  per-lane statuses with manual actions, driver inventory, protocol
  revisions, implementation status, evidence locations.
- `crates/vesper-bridge/tests/contract_invariants.rs`: 4 new tests —
  idempotency-class totality/distinctness, the full NF-10 retry matrix
  across every `BridgeError` variant, capture-identity durability cases
  (reuse/recompute/no-serial asymmetry), and settled/success outcome
  classification.

Gates:
- `cargo test -p vesper-bridge` → **50 passed** (29 + 17 + 4).
- `cargo clippy -p vesper-bridge --all-targets -- -D warnings` → clean.
- `cargo +1.88.0 check -p vesper-bridge --tests` → clean.
- `cargo fmt` → clean; `cargo xtask naming-guard` → clean.

## Files changed

- `docs/Vesper bridge/recon/cua-driver-provenance.md` (new),
  `compatibility-manifest.md` (new), `cua-0.28.1-checksums.txt` (pinned),
  `SOURCE-DIGESTS.txt` unchanged (new file listed below).
- `crates/vesper-bridge/tests/contract_invariants.rs` (new).
- Evidence index updated.

## Exact evidence

| Item | Test/observation | Result |
|---|---|---|
| Driver digest pinned before any install | `recon/cua-driver-provenance.md` + pinned checksums file | DONE (no install performed) |
| Prerelease status recorded | release API JSON (prerelease: true) | DONE |
| NF-10 retry matrix across all errors | `error_retry_matrix_matches_nf10` | PASS |
| Idempotency classes total & distinct | `idempotency_classes_are_distinct_and_total` | PASS |
| Capture identity reuse cases | `capture_identity_durable_rules_cover_the_reuse_cases` | PASS |
| Outcome classification | `settled_outcomes_are_terminal_and_success_is_verified_only` | PASS |

## Honest status

- No lane advanced from BLOCKED: Resolve is still not installed and Cua
  is still not authorized. Those are the two manual actions standing
  between the current state and Phases 3/4 (listed in the manifest).
- Native enrollment retried this turn: the gate now fails at
  "independent review timed out" — past the repaired paragraph ceiling,
  inside the window, at the live-model reviewer step. Environmental;
  documented in the intake note. No reduced scope was enrolled.
- All 44 AT rows keep their statuses; the four new tests strengthen the
  Phase 1 contract-evidence class only.

## Deviations

- None.

## Unresolved items

1. Authorization: Cua 0.28.1 install (Alex) — Phase 4 unblocks.
2. Installation: DaVinci Resolve Studio (Alex) — Phase 3 unblocks.
3. Native enrollment (host restart on the repaired binary).
4. Host-startup OS observation for AT-01.

## Readiness effect

Phase 4's supply-chain prerequisite is now complete on our side: the
moment authorization exists, installation is a digest-verified,
manifest-scoped step with the lane decision already recorded. Contract
coverage is at 50 deterministic tests.
