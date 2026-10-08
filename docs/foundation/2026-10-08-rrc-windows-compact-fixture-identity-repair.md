# RRC Windows compact fixture identity repair — 2026-10-08

## Objective and status

Repair the single admitted `five-target-foundation / windows-x86_64` failure in
`isolated_agent_repair_verifies_and_promotes_one_patch_for_two_provider_fixtures`
without weakening isolated proof, provider-fixture, or promotion coverage.

**Status at report freeze:** source and required documentation are complete. Per
the repair-controller contract, the one focused verification command runs only
after the final edit; its result is delivered with this report and must not be
inferred from this pre-verification record. Native Windows exact-SHA rerun remains
pending.

## Causal family and repair hypothesis

There is one admitted family: the Windows isolated-repair composition fixture.
The hosted panic was:

```text
thread 'release_executor::tests::isolated_agent_repair_verifies_and_promotes_one_patch_for_two_provider_fixtures' (5520) panicked at crates\vesper-harness\src\release_executor.rs:8529:18:
called `Result::unwrap()` on an `Err` value: Invalid("script exhausted [category=InvalidRequest, HTTP=unavailable, retry=Never, retry-after-ms=None]")
```

`script exhausted` means the scripted repair issued an additional provider
request after its write, focused Cargo test, and final response. In this path that
continuation occurs when the focused command does not establish successful
post-edit proof. The failure was isolated to Windows after the fixture package
was changed from the short shared name `repair-composition` to
`repair-composition-<16 hex>` to prevent shared-target artifact aliasing.

The test-only ungoverned Cargo target is nested under the RRC state root,
`worktrees/<64-hex repo digest>/<timestamped repair leaf>/...`. Cargo incorporates
the package/target name into Windows test-executable paths. Expanding that name by
17 characters consumed scarce Windows path headroom while Linux remained green.
The evidence supports one clustered hypothesis: the uniqueness repair made the
already-deep Windows fixture proof path too long, the focused Cargo command did
not settle as a success, and proof continuation exhausted the three-response
script.

The correction preserves a distinct 64-bit-equivalent hexadecimal prefix per
fixture but uses the compact Cargo package identity `r<16 hex>`. This restores the
prior target-stem length while retaining the existing shared-target regression
that compiles both independent projects before running either binary and asserts
different executable paths. No production worktree, governor, retry, proof, or
promotion behavior changes.

The hosted log does not include the failed Cargo command's own diagnostic, so
path exhaustion is classified as strongly supported rather than an exact local
Windows reproduction. The focused local result cannot substitute for the pending
native Windows rerun.

## Methods and commands

Inspected:

- the admitted Windows panic and failing source line;
- `assert_isolated_repair_promotion` and its two-provider/two-route matrix;
- scripted repair response construction and proof-continuation behavior;
- isolated repair worktree path construction;
- the recent unique-package shared-cache regression and its retained rationale;
- applicable root, harness, documentation, and foundation DOX contracts.

Applied one causally scoped source edit:

```text
repair-composition-<16 hex>  ->  r<16 hex>
```

Final focused command selected for post-edit execution:

```text
cargo test -p vesper-harness isolated_agent_repair_verifies_and_promotes_one_patch_for_two_provider_fixtures -- --exact
```

This exact test exercises both registered fixture provider IDs and both remote and
preparation promotion routes, including real isolated Git worktrees and real
post-edit Cargo proof. It is the smallest single command covering the admitted
family. Its delivery-time exit status and test count are authoritative; this
report intentionally does not predeclare a pass.

## Files

Changed source:

- `crates/vesper-harness/src/release_executor.rs` — compact unique fixture Cargo
  identity.

Updated contracts and evidence:

- `crates/vesper-harness/AGENTS.md`
- `docs/AGENTS.md`
- `docs/foundation/AGENTS.md`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/2026-10-08-rrc-windows-compact-fixture-identity-repair.md`

Root `AGENTS.md` is intentionally unchanged: the provider-neutral RRC and
cross-platform proof contracts already apply, and this repair changes only the
harness fixture's compact path representation. No child index changed because no
file or ownership boundary moved.

## Exact evidence

Current admitted evidence consists of one Windows 2025 failure fingerprint:

```text
a57b130e43e489132e4505474c3ce6a87092eedaf494151d0fd276d3e602cbc1
```

The failure occurred in `Run eligible foundational tests`; the exact named test
panicked on `script exhausted`. There is no prior repair evidence in the admitted
prompt, and no prior progress is counted as current proof.

Source-level invariants retained:

- package identities still derive from the full workspace-path digest;
- the manifest stores the first 16 hexadecimal digest characters;
- copied Git repair worktrees retain that identity;
- the existing alias regression still requires distinct executable paths and
  successful execution of both compiled test binaries;
- the failing composition test still runs two provider fixtures across both
  preparation and remote routes.

## Deviations and unresolved items

- The original Windows Cargo stderr/stdout preceding proof continuation was not
  present in the admitted log, so this report does not claim an exact reproduced
  `filename too long` diagnostic.
- No Windows runner is available inside this bounded isolated repair worktree.
  The required fresh exact-SHA Windows matrix remains unexecuted.
- No broad acceptance, workspace, MSRV, release, publication, or live-provider
  suite is in scope. The selected exact test is the only post-edit verification.
- No commit, push, tag, publication, remote mutation, extra worktree, release
  state edit, installer, or user-state write was performed.

## Readiness effect

The repair removes the Windows-specific path expansion while retaining the
artifact-isolation behavior that motivated unique fixture identities. A successful
focused command establishes local coverage of all four provider/route fixture
combinations, but only a fresh native Windows exact-SHA pass can close the admitted
platform failure. Release readiness therefore remains pending that hosted result
and all other controller-owned gates.
