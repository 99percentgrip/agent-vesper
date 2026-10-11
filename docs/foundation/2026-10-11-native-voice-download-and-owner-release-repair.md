# Native voice download and controller owner-release repair

## Objective and status

Finish the approved Windows/macOS voice repair and single v0.24.12 native release. Correct the complete failed ConPTY candidate's Windows download helper and RRC ownership lifetime. Current local red/green proof passed; fresh native exact-SHA CI and publication remain pending.

## Complete failed candidate evidence

Candidate `dd898eb391110f5bd873a99d09b47a12bfb8623a` passed all eight native local gates. All four required hosted workflows settled, with ten of twelve jobs green:

- Canonical quality, Windows installer, supply-chain, MSRV, three contained-web jobs, Linux x64, macOS ARM and Intel macOS passed.
- Linux ARM job `114443209980` in run `38130353425` failed `controller_owner_is_exclusive_across_independent_ledger_handles`: immediate acquisition after dropping the original file owner remained busy.
- Windows job `114443210125` in that run passed first-launch/provider Settings, the ConPTY cleanup guard and native release host controls. Production dependency setup verified Python and espeak. Production recognition returned the fixed phrase twice with ordered ready/chunk/done receipts, proving warm reuse without a microphone/provider.
- Windows then failed Natural Voice at the first asset: `Voice pack storage error: could not start the download (entity not found)`. Production `download_to` hardcoded `/usr/bin/curl`; this is source portability evidence, not a network outage. Windows synthesis did not execute.
- RRC recorded both causes and stopped `NeedMoreEvidence`, with zero repairs/full retries. The missing-helper diagnostic was incorrectly unknown. No tag/publication occurred.

[Complete matrix and preserved native state](2026-10-11-native-voice-download-and-owner-release-repair-failed-matrix.json).

## Methods and commands

All Cargo checks used the locked dependency graph, jobs=1, test threads=1, incremental disabled and the managed target cache.

1. Added a deterministic duplicated-file-description regression to the original owner API. It reproduces the exact failed acquisition predicate with zero timing assumptions: one executed test failed before repair. A retained open description explains how a concurrent fork can prolong a flock, but the ARM log does not prove which descendant retained a descriptor.
2. Added the actual native missing-helper log to classification/extraction regression: one executed test failed Unknown versus expected DependencyFailure. The test also preserves owner-action priority and unknown generic exits/storage text.
3. Extracted the existing hardcoded downloader selection without changing behavior, then asserted both OS selections: one executed test failed Unix path versus Windows executable. This is portable selection proof, not native execution of `curl.exe`.
4. Added an echo-only adversarial case; the initial substring matcher failed one executed case. A full rerun retained that failure (103 passed, 1 failed) because the textual replacement had not changed the matcher. Applied the explicit correction, observed one exact green test, then reran all 104 cases successfully. Recognition now requires exact normalized native error lines; no command echo alone grants diagnosis.
5. After final repair, `cargo test --locked -p vesper-harness --lib --all-features release_recovery::tests`: **104 passed, 0 failed**, including both new RRC cases and original exclusivity case.
6. `cargo test --locked -p vesper-voice-kokoro --features ort,mock-synthesis`: **45 passed, 0 failed**. Bare crate: **9 passed, 0 failed**.
7. Clippy for both changed crates, all targets/features, `-D warnings`: exit 0. Formatting, new links/JSON/archive hashes and whitespace checked.

[Digest manifest](2026-10-11-native-voice-download-and-owner-release-repair-receipts.json) and [exact local/failed-job receipts](2026-10-11-native-voice-download-and-owner-release-repair-receipts.tar.gz).

## Files and behavior

- `crates/vesper-harness/src/release_recovery.rs`: owner guard explicitly unlocks before closing so retained duplicate/inherited descriptions do not extend a settled lease. Exclusive acquisition while the guard lives remains required. Unknown unlock failure is not converted into acquisition authority: subsequent acquisition still uses the OS lock.
- The same shared module recognizes only the existing exact native voice-pack missing-download-helper diagnostic as a strongly supported dependency failure. It retains uncertain generic exits and other storage messages; no OS label manufactures certainty. Both hosts use this shared path, with no provider-specific branch.
- `crates/vesper-voice-kokoro/src/setup.rs`: selects `curl.exe` on Windows and `/usr/bin/curl` on Unix. HTTPS-only source/redirects, transfer bounds, resumable staging and immutable size/hash verification remain enforced.
- Owning contracts, both PRDs, evidence index and source provenance are included in this corrective code commit. Parent root/crates/app/CI contracts are unchanged because registration, permissions, hierarchy and workflow coverage are unchanged.

## Resource and release boundaries

During the prior native candidate, workspace verification passed before disk reserve paused acceptance. Operator cleanup removed only historical managed debug incremental snapshots with all metadata older than 09:07 UTC and no active compiler. It restored 133.22 GiB free; the same worker automatically resumed acceptance, preserving the passed gate and retry counts. This operator action is not hands-free cache recovery evidence. Its exact receipt is archived here.

No ledger edits, budget resets, extra version bump, release tag or installed application update were performed. Prior epochs and failed receipts remain preserved through normal canonical-source reconciliation. These source repairs were operator-performed; they do not prove that RRC autonomously fixes every possible failure. The narrow diagnosis correction supports its future missing-helper repair path without weakening unknown-cause refusal.

## Unresolved items and readiness effect

Fresh native platform acceptance must verify actual Windows pack downloads, extraction, setup and both synthesis signals, plus the repaired ownership behavior. Previous native recognition/macOS receipts do not certify changed source. Microphone, speakers, OS permission prompts, the original Windows laptop and unlimited dictation remain separate/unexecuted acceptance. RRC owner-loss fences on uncertain unfinished repair work remain unchanged. This report certifies local corrective proof and readiness to enter exact-SHA CI for unchanged **0.24.12**, not complete PRD parity or publication.
