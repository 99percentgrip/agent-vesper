# RRC adaptive resource admission repair

## Objective and status

Correct the native v0.24.8 candidate's prolonged resource wait without weakening safety limits, changing provider behavior, resetting retry history or replacing Alex's installed application. Work occurs in isolated `/tmp/vesper-rrc-closeout-fix`, branch `repair/rrc-automatic-closeout`, parent `8ef28db4c9710da3e65e650d9266a2f025f9f9bc`. Shared source and focused tests are repaired; rebuilt native progression and final versioned local/hosted release gates remain pending. No tag or public release created.

## Methods and findings

Read the live ledger and owner process after Alex reported no progress. At 12:33:28 UTC, resource observations were fresh, but version preparation had waited since 12:08:24 UTC with 55 unchanged observations and zero gates or mutations. Measured RAM was about 18.38 GiB, reserve about 6.76 GiB, full logical swap was stable (zero growth), memory PSI was zero and physical zram backing about 3.69 GiB. Disk headroom was sufficient.

The governor classified the maximum affordable two-job budget first, then reported one job because that classification was Pressure. Its doubled near-full-swap allowance therefore still required about 18.76 GiB for two jobs while the proposed one-job policy could satisfy every existing margin. No automatic lower-budget admission was attempted. My earlier capacity-only explanation missed this implementation gap.

`telemetry_from` now evaluates bounded Cargo job budgets from the memory/CPU limit downward and selects a lower budget only if the complete unchanged pressure classifier returns Normal. Desktop reserve, minimum normal headroom, per-job 3-GiB estimate, doubled near-full-swap margin, owned-tree margin, PSI, swap-growth, zram, critical stops, disk reserve and exclusive scheduler are preserved. Exported Cargo jobs and displayed gate headroom use that same selected budget. Both hosts inherit this shared provider-neutral implementation with no provider or host match arm.

The native `/release cancel` command stopped the waiting epoch and preserved its checkpoint; the temporary native TUI was then closed. A shutdown input mistake using unsupported Ctrl+U prefixed a command and briefly started an unintended ordinary prompt; it was cancelled immediately and no source change was observed. This runtime operator error is not foundation-test evidence. No user TUI, other application, credential, installer or retry budget was modified.

## Verification receipts

- Red regression `stable_full_swap_admits_one_job_when_two_job_margin_does_not_fit`: actual production classifier returned Pressure; one failed case preserved in `/tmp/vesper-rrc-adaptive-admission-red.log`.
- Initial repaired governor suite: 19 passed, zero failed (`/tmp/vesper-rrc-adaptive-admission-green.log`).
- Final governor suite: 21 passed, zero failed (`/tmp/vesper-rrc-adaptive-admission-final-v2.log`). New cases prove admitted one-job environment, refusal under low/critical RAM, PSI, swap growth, zram backing, oversized owned tree and insufficient disk, plus CPU-capped admission consistency. Existing threshold fixture now uses RAM below the one-job margin and sizes the owned tree independently, preserving its genuine unsafe-state assertions.
- One intermediate test compile failed because its repeated HostCapacity fixture was moved rather than cloned; preserve `/tmp/vesper-rrc-adaptive-admission-final.log`. Corrected fixture uses Clone; no production threshold changed.
- Strict all-target/all-feature harness Clippy passed (49.89 seconds); rebuilt production TUI passed (42.20 seconds). Final versioned repository gates and exact-main prerequisite/producing workflows remain required. These focused tests do not certify all PRD parity or a public release.

## Files and DOX

`crates/vesper-harness/src/host_resources.rs`, nearest harness contract, this report and receipt companions, foundation ownership/evidence index, owning RRC PRD, and canonical integrated objective provenance. Root/crates/docs/apps hierarchy and dependency ownership are unchanged, so their parent contracts remain unchanged after the DOX pass. This documentation stays in the same code candidate before CI; no separate post-release documentation push.

## Deviations, unresolved items and readiness effect

The initial native wait proved lifecycle ownership but did not run final gates. Its measured-capacity receipt remains historical. The repaired host must automatically admit one job on actual safe live telemetry, then complete version preparation, full local checks, exact-main canonical/MSRV/five-target/web-driver workflows, immutable tag/publication, registry and completion receipt. Real unsafe external pressure still defers; no automatic permission escalation or safety bypass is authorized. Installed application remains v0.24.7 and requires Alex's separate update after publication. No independent delegated review or live-model repair effectiveness is claimed.

## Receipt binding

[Evidence JSON](2026-10-07-rrc-adaptive-resource-admission-repair-evidence.json) binds focused source and rebuilt host hashes; [raw receipts](2026-10-07-rrc-adaptive-resource-admission-repair-receipts.tar.gz) preserve failures, successes and the earlier native wait. Archive SHA-256: `7a99cffccfeb785fd23ef0cd4060137ee2759fb385d04e5f37b4a993ed6f6656`. Rebuilt execution-only host SHA-256: `d5c3e1ed3eed2064a49305944b9d643a467bf6c506d19d1fca7b4ef6a61793e4`.

Before final native progression, preserved the rebuilt execution-only host in `/tmp` and cleaned only the inactive RRC-managed compiled cache with `cargo clean --target-dir <exact-controller-cache>`, holding its exclusive scheduler lock and observing no Cargo/rustc/ACP process. This reserves headroom for all versioned gates; it does not delete source, user state or receipts. Log `/tmp/vesper-rrc-adaptive-inactive-cache-clean.log`.
