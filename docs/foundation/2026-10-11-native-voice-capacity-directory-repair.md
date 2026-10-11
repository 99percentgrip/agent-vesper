# Native voice capacity directory repair — 2026-10-11

## Objective and status

Repair the Windows production capacity-validation defect found while releasing
the approved native voice implementation. Preserve version **0.24.12**, pinned
runtime assets, native Settings setup, provider neutrality, RRC retry limits and
Alex's existing installation and original dirty checkout.

**Status:** portable production correction and local regression proof complete;
fresh exact-commit release gates and device acceptance remain separate. This
report is included with the corrective source before CI, not pushed after a green
release as another documentation-only release.

## Causal evidence

Candidate `565de71f08827169026babb22e9bd8cd5191298f` failed the Windows
foundation job [114395755459](https://github.com/99percentgrip/agent-vesper/actions/runs/38114213471/job/114395755459):

```text
setup::tests::native_capacity_handles_missing_descendants_and_refuses_invalid_paths ... FAILED
crates\vesper-voice-kokoro\src\setup.rs:996:9:
assertion failed: free_bytes(&invalid).is_none()
test result: FAILED. 42 passed; 1 failed
```

The invalid path is a child beneath an existing regular file. Windows reports
that lookup as `NotFound`; the capacity helper ascended to the file and accepted
its filesystem capacity. Linux reports a non-directory error for the child, so
the original test passed locally. However, an existing file directly also passed
the defective helper on Linux, providing a portable red regression for the same
production branch. The failed path contains no NUL byte; an initial diagnostic
guess was corrected immediately after reading the actual fixture.

## Methods and changes

- Inspected the exact Windows log, `setup.rs::free_bytes`, its regression,
  `VoicePackSetup::plan`, the TUI capture-store capacity helper and applicable DOX.
- Strengthened the existing test to reject an existing file and descendants one
  and two levels beneath it. Its successful missing-directory capacity assertion
  remains intact. Each invalid root must also make setup planning return Storage.
- Reproduced one selected failure before changing production behavior.
- Require `metadata.is_dir()` before querying filesystem capacity. An existing
  non-directory immediately returns unavailable capacity; only `NotFound`
  permits ascent. Permission and other errors still fail closed.
- The corrected branch matches the existing TUI capture-store implementation.
  No new provider-specific path, OS installation or runtime package is introduced.

## Complete prior candidate matrix

The [immutable failed-matrix receipt](2026-10-11-native-voice-capacity-directory-repair-failed-matrix.json)
retains all four exact-SHA workflow results and all twelve terminal jobs:
**11 success, 1 failure**. Canonical, MSRV and web-driver workflows passed;
foundation failed only on Windows capacity validation. Both Linux architectures
and both Mac architectures passed their complete foundation lanes.

The Intel and Apple Silicon logs each record verified native speech dependencies,
production recognition and worker reuse, and nonzero real synthesis for both
`am_michael` and `af_heart`. The Windows installer job independently passed real
release executable launch, signed-out landing/all four providers/auth Back and
static-CRT import checks. Its successful installer proof does not clear the
failed Windows foundation job or prove Windows voice setup.

The native controller settled with `liveness.state = failed`,
`settled_error = true`, no in-flight operation and no tag. Its explicit result
was `retry blocked: full-gate budget exhausted; further repair promotion forbidden`.
The stop is bounded and truthfully incomplete, rather than a continuing loop.

Changed files: `crates/vesper-voice-kokoro/src/setup.rs`, its owning `AGENTS.md`,
this report and receipt bundle, foundation/documentation ownership and evidence
index, voice/RRC PRD evidence links and release-objective provenance.

## Exact local verification

Commands use locked dependencies, one build job, one test thread, disabled
incremental compilation and the controller-managed target cache. They perform no
live provider requests, setup downloads, user-state writes or audio-device access.

| Check | Command | Result |
| --- | --- | --- |
| Red | `cargo test --locked -p vesper-voice-kokoro --features ort,mock-synthesis setup::tests::native_capacity_handles_missing_descendants_and_refuses_invalid_paths -- --exact` | Exit 101; **0 passed, 1 failed, 43 filtered**. The existing-file assertion fails before the production fix. |
| Green | `cargo test --locked -p vesper-voice-kokoro --features ort,mock-synthesis` | Exit 0; **44 passed, 0 failed**. |
| Bare | `cargo test --locked -p vesper-voice-kokoro` | Exit 0; **9 passed, 0 failed**. |
| Clippy | `cargo clippy --locked -p vesper-voice-kokoro --all-targets --all-features -- -D warnings` | Exit 0; 13.67 seconds. |

The [receipt JSON](2026-10-11-native-voice-capacity-directory-repair-receipts.json)
and [archive](2026-10-11-native-voice-capacity-directory-repair-receipts.tar.gz)
retain command identity, log hashes and
nonzero red/green results. Broader verification and native Windows branches are
not inferred from these Linux-local receipts; RRC must run fresh production gates
and the complete hosted matrix against the corrective commit.

## Native RRC observation and deviations

Before this new defect, the native RRC itself repaired the Windows/macOS stale
`aplay` assertions. Its recorded focused proof was
`cargo test -p agent-vesper-tui --features voice-kokoro --test r3_voice_pack --test pr4_f9_gate`.
Independent production verification passed before it promoted and pushed
`565de71f08827169026babb22e9bd8cd5191298f`. Two resource pauses retained the
same prepared patch and admissions, recovered automatically after capacity
returned and did not create failed-source hypotheses or additional model turns.
Root orchestration reclaimed only verified historical controller-managed
incremental caches while compilers were stopped; host capacity recovery is not
claimed to be entirely hands-free.

The persisted full-matrix retry is now **1/1**. This corrective source unit does
not clear or reset the exhausted epoch. Natural native admission of the updated
canonical source must retain/archive the prior evidence through the existing
candidate-reconciliation contract. No identical failed matrix is blindly rerun.
The same version remains unpublished until every required exact-SHA workflow
passes, and publication is distinct from registry/report/controller completion.

## DOX pass and unresolved boundaries

The nearest Kokoro DOX records the non-directory ancestor rule. Documentation and
foundation ownership, evidence index and owning PRDs link this report. Root and
crates parent contracts remain unchanged intentionally: dependencies, ownership
boundaries, permissions, retry policy and child indexes are unchanged.

Original Windows laptop microphone/speaker permissions, live capture, audibility,
latency and macOS hardware acceptance remain unexecuted here. The existing
120-second/4-MiB capture limit remains; this is not unlimited-dictation evidence.
The new candidate still requires canonical, MSRV, five-target foundation and
contained web-driver gates, then immutable tag/assets and native registry/report
closeout. No installer is run on Alex's machine.
