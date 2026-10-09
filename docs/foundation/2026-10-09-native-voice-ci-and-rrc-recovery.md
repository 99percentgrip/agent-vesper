# Native voice CI and RRC recovery — 2026-10-09

## Objective and status

Continue the approved Windows/macOS voice repair and the **same v0.24.12**
release through native RRC. Correct the shared voice compile regression and
failed source-repair journal without weakening ownership, permission, evidence
or retry fences. The [implementation report](2026-10-09-native-voice-platform-parity-repair.md)
owns the eight approved platform/setup repairs.

Status: corrective source and local proof completed; exact-source native CI
and release completion pending. No target tag or publication is certified here.

## Observed failure

The [retained failed matrix](2026-10-09-native-voice-ci-and-rrc-recovery-failed-matrix.json)
contains exact jobs, causal families and unchanged admissions before corrective
source adoption. It is historical evidence, not proof of the next candidate.

- Implementation source: `037e3677e73de53eff3767ec2f620fb96cc6ec69`.
- Native RRC prepared/pushed `251e89ce9d6122b785d45de87e289cf63d144778`
  for v0.24.12. All twelve prerequisite jobs settled before diagnosis.
  Canonical quality/supply chain, MSRV, both Linux foundation targets and all
  web-driver jobs passed. Windows installer/foundation and both macOS foundation
  jobs failed; their downstream speech checks were unexecuted.
- The four failures share `E0599` at `voice.rs:957`: the shared response reader
  calls `Read::take`, but its trait import was Linux-only. Earlier isolated
  native-audio checks omitted this reader. Full native app CI blocked tagging.
- Exact runs: [canonical](https://github.com/99percentgrip/agent-vesper/actions/runs/37927134776),
  [MSRV](https://github.com/99percentgrip/agent-vesper/actions/runs/37927134577),
  [five targets](https://github.com/99percentgrip/agent-vesper/actions/runs/37927134639),
  [web driver](https://github.com/99percentgrip/agent-vesper/actions/runs/37927134512).
- RRC reserved one repair admission for each of four causal families. The selected
  provider returned HTTP 429 without successful repair/proof. The worker persisted
  Failed liveness but returned before clearing its `ClassifyingFailure` journal;
  restart with another registered provider was blocked by that stale journal.
  Controller failure, provider rejection and GitHub billing are distinct.
- Both old TUI owners were quit cleanly. The original dirty checkout, installed
  app and release ledger were not reset.

## Changes and proof

| Owner | Correction | Proof |
| --- | --- | --- |
| TUI `voice.rs` | Import `Read` for every target; retain bounded reader/error policy | Actual production import/expression fails before and compiles after for Windows MSVC and both macOS architectures |
| Shared `release_executor.rs` | Settle failed isolated source repair only when complete native Git evidence proves the exact pushed candidate is clean and unchanged, with no tag/publication state | Red-first real-Git regression; reloaded ledger preserves failures, admissions and budgets |
| Shared worker restart | Under exclusive owner lock, require typed returned-error metadata and reconcile before heartbeat | Same proof; owner exit, legacy missing metadata and uncertain mutation remain fenced through repeated requests |
| `xtask` | Require all three journal/restart cases on every native target | 162/162 local acceptance passed |
| Native voice CI | Require fixed-phrase recognition twice through the production sidecar, plus setup and synthesis | Linux real CPU int8/VAD passed; all five hosted lanes must execute it |

Recovery does not infer repair success or replay a release write. Failed isolated
siblings and reserved admissions remain. Sixteen refusal cases cover dirty
tracked/index/untracked state, changed HEAD, mismatched candidate/evidence,
infrastructure diagnosis, missing admission, cancellation, another journal and
tag/publication state. Both hosts use the same worker, without provider-name logic.
The registered-worker fixture additionally checks recovery before heartbeat,
continued permission refusal and repeated owner-exit/legacy/uncertain-write refusal.
Optional `settled_error` liveness metadata defaults false for legacy ledgers;
blocked retries cannot turn uncertainty into settlement evidence. Controller
panics are mutation-blocked uncertainty. A newer canonical strict descendant
can supersede an obsolete prerelease source only after archiving its full record;
this remains the safe route for the original opaque legacy checkpoint.

## Methods and exact receipts

The [local receipt manifest](2026-10-09-native-voice-ci-and-rrc-recovery-local-receipts.json) binds the retained
[compressed logs](2026-10-09-native-voice-ci-and-rrc-recovery-local-receipts.tar.gz). Restart guard mutation failed
the owner-exit assertion; restoring the typed-metadata guard passed the exact test.

Cargo uses two jobs, one test thread, no incremental compilation and debug info
disabled. Foundation tests use temporary Git/ledger roots and offline providers;
no live provider calls or user-state writes occur in verification.

- Red: `cargo test -p vesper-harness --lib --all-features failed_source_repair_journal -- --nocapture`
  failed the clean-candidate settlement assertion with the helper disabled.
- Green: the same command passed both tests. The refusal matrix was then expanded
  to sixteen cases; all three focused cases and the full 162-case acceptance passed.
- Native probe copies the actual production import and bounded reader expression.
  `rustup run 1.95.0 rustc --crate-type lib --target <target>` returned 1 before
  and 0 after on Windows MSVC, Intel macOS and ARM macOS. This proves that
  expression's compile correction, not full app/runtime/hardware acceptance.
- `cargo xtask verify` passed formatting, strict workspace Clippy, all-feature
  workspace tests and its loaded 159-case acceptance set. After the restart
  metadata test was added, current-source focused tests passed 3/3, strict
  workspace Clippy passed, and `cargo xtask acceptance` passed **162/162**.
- Current Rust 1.88 `cargo test --locked --workspace --all-features --all-targets`
  passed, including all three new regressions. Shipping-feature TUI build passed.
- The changed hosted setup example compiles with Rust 1.95 and 1.88.
- Offline native-voice guards passed 4/4, including missing/misleading/reordered
  recognition receipts. Python 3.12/faster-whisper 1.2.1 in an owned temporary
  environment ran actual CPU int8/VAD and the production sidecar twice:
  `This is a speech recognition test. Hello world.` Both chunks/done receipts
  passed. The fixture's four immutable model files were size/SHA-256 verified;
  the successful rerun reused those verified bytes. No microphone, provider or
  real credentials were used. Model/default accuracy is not established.
- The first diagnostic probe used the library's file decoder and failed because
  current PyAV rejects `metadata_errors`. Production feeds NumPy PCM instead;
  the corrected probe follows that actual path and passed. This remains a failed
  diagnostic receipt, not a claimed production transcription defect.
- Fixture provenance: [immutable Systran model](https://huggingface.co/Systran/faster-whisper-tiny.en/tree/0d3d19a32d3338f10357c0889762bd8d64bbdeba)
  and [upstream CPU/VAD documentation](https://github.com/SYSTRAN/faster-whisper).
- Corrective exact-source release: pending. Failed earlier evidence is retained.

## Files and DOX pass

Source: `apps/agent-vesper-tui/src/voice.rs`,
`crates/vesper-harness/src/release_executor.rs`,
`crates/vesper-harness/src/release_recovery.rs`, `xtask/src/main.rs`, the hosted
voice receipt example and `.github/` recognition script, guards and pinned fixture.
Owning contracts now describe shared-reader native compilation and safe failed
source-repair settlement/acceptance. The evidence index, both voice PRDs and RRC
PRD link this report. Canonical release provenance must bind verified descendant
source before CI. Root/apps/crates/docs parents retain existing boundaries and
child indexes; ownership is unchanged and duplicate rules are unnecessary.

## Unresolved acceptance and readiness effect

- Corrected exact-source canonical/MSRV/five-target/web-driver matrices, producing
  workflow/assets/checksums and existing Registry PR closeout are required.
- Windows/macOS microphone permissions, F5 transcript, F9 audible output,
  Stop/interruption and Settings OS approvals need native device tests. Runner
  synthesis cannot certify the original Windows laptop or human audibility.
- Existing R20 capture quotas (120 seconds / 4 MiB per clip) remain separate;
  this repair does not establish unlimited dictation.
- GLM rejection remains a failed live repair attempt; fixtures/journal recovery
  do not establish universal live-model repair effectiveness.
- Local fixes or an older passing release do not certify installed parity. These
  checks cannot support a universal absence-of-bugs or 100% device-parity claim.
