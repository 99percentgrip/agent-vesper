# Native voice five-target foundation repair — 2026-10-10

## Objective and status

Repair every causal family admitted by the bounded Release Recovery Controller
(RRC) turn: one Windows interpreter-precedence test failure and one shared macOS
Natural Voice retained-size assertion failure observed on both native macOS
architectures. Keep the correction confined to the failed tests and their
budget documentation; do not change production interpreter precedence, pinned
pack assets, setup budgets, release state, remotes, or publication state.

**Observed follow-up:** the native repair agent completed the source and DOX
changes and successfully ran `cargo test -p agent-vesper-tui -p
vesper-voice-kokoro`. RRC independently repeated focused verification before
canonical verification. The canonical run was stopped by critical swap growth;
RRC incorrectly recorded this resource interruption as a disproven hypothesis
and dispatched a second repair. That attempt was cancelled before promotion.
The full failed/cancelled checkpoint is retained in
[the resource-failure receipt](2026-10-10-rrc-prepared-repair-resource-failure.json).
The [resource-recovery continuation](2026-10-10-rrc-prepared-repair-resource-recovery.md)
owns the corresponding controller correction and subsequent validation.
Neither the local package result nor the interrupted canonical run establishes
fresh Windows/macOS release readiness.

## Admitted evidence and causal clustering

No prior repair evidence was supplied. The controller admitted three distinct
fingerprints, which reduce to two first-cause families:

1. **Windows interpreter fixture construction** — fingerprint
   `0e54c006edee8ef3cbae1374c837af958cf52f52f88781e7f428f382d29fbee7`.
   The admitted Windows log reports:

   ```text
   thread 'tests::vesper_python_interpreter_resolves_env_var_precedence' ... panicked at apps\agent-vesper-tui\src\main.rs:18875:62:
   called `Result::unwrap()` on an `Err` value: Os { code: 3, kind: NotFound, message: "The system cannot find the path specified." }
   ```

   Source inspection showed that the test created only `<temp>/bin`, while
   `platform_voice::venv_python` correctly returns
   `<temp>/Scripts/python.exe` on Windows. The subsequent fixture write therefore
   targeted a missing parent directory before precedence behavior was exercised.

2. **macOS target-specific retained pack size** — fingerprints
   `9af00990c3f953a915907b0bab3acc9781710b723dd03de12e63df056deaa18b`
   and
   `ed28ed0d40058808e159de67e48dda3c282195a0c67743f79e40cad0dc554999`.
   Both admitted macOS logs fail the same compile-time test assertion:

   ```text
   assertion failed: RETAINED_PACK_BYTES < 120 * 1024 * 1024
   --> crates/vesper-voice-kokoro/src/pack.rs:806:23
   ```

   The pinned ARM macOS manifest retains `133,047,356` bytes and the pinned
   Intel macOS manifest retains `133,479,640` bytes. Both legitimately exceed
   the stale `125,829,120`-byte host-specific band while remaining far below the
   authoritative `RETAINED_BUDGET_BYTES` ceiling of `268,435,456` bytes. The
   two fingerprints are therefore one causal family, not two independent pack
   defects.

## Repair

| Family | File | Causally relevant correction |
| --- | --- | --- |
| Windows interpreter fixture | `apps/agent-vesper-tui/src/main.rs` | Use an owned `tempfile::TempDir`, resolve the fixture path through the same native `venv_python` helper under test, create that resolved path's parent, and pass the root as an `OsStr`. This creates `Scripts/python.exe` on Windows and `bin/python` elsewhere without changing precedence behavior. |
| macOS retained pack budget | `crates/vesper-voice-kokoro/src/pack.rs` | Remove only the obsolete `100–120 MiB` descriptive band. Retain the authoritative retained/peak budget assertions and exact `PEAK_SETUP_BYTES - RETAINED_PACK_BYTES == RUNTIME_ASSET.size` relationship. Correct the module comment to the measured supported-target range of roughly 105–128 MiB. No asset, digest, runtime, setup, or budget constant changed. |

## Methods, files, and exact evidence

Inspected:

- `apps/agent-vesper-tui/src/main.rs` — failing test and adjacent interpreter
  candidate coverage.
- `apps/agent-vesper-tui/src/platform_voice.rs` — native venv layout:
  `Scripts/python.exe` on Windows and `bin/python` elsewhere.
- `crates/vesper-voice-kokoro/src/pack.rs` — all five target manifests,
  retained/peak formulas, authoritative budgets, and failing assertion.
- Applicable root, app, crate, documentation, foundation, test, and example
  `AGENTS.md` contracts.

Changed source:

- `apps/agent-vesper-tui/src/main.rs`
- `crates/vesper-voice-kokoro/src/pack.rs`

Changed records:

- `docs/foundation/2026-10-10-native-voice-foundation-ci-repair.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/AGENTS.md`
- `docs/AGENTS.md`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`

Arithmetic receipts from the pinned source constants:

```text
macOS ARM retained = 92,361,116 + 522,240 + 522,240 + 3,497
                     + 39,312,136 + 1,073 + 325,054
                   = 133,047,356 bytes
macOS Intel retained = 92,361,116 + 522,240 + 522,240 + 3,497
                       + 39,742,608 + 1,073 + 326,866
                     = 133,479,640 bytes
stale narrow ceiling = 120 * 1,048,576 = 125,829,120 bytes
authoritative ceiling = 256 * 1,048,576 = 268,435,456 bytes
```

Originally planned focused command (rejected by the native command policy before execution):

```text
cargo test -p agent-vesper-tui --bin agent-vesper-tui tests::vesper_python_interpreter_resolves_env_var_precedence -- --exact && cargo test -p vesper-voice-kokoro pack::tests::budgets_headroom_is_honest -- --exact
```

The accepted command was `cargo test -p agent-vesper-tui -p
vesper-voice-kokoro`; its successful native tool receipt is preserved in the
checkpoint. The controller then began canonical verification, which did not
complete. No tag, publication or installation resulted from this attempt.

## DOX pass

`docs/foundation/AGENTS.md` and `docs/AGENTS.md` now own and index this report;
the foundation evidence index and RRC PRD link it. The nearest app and Kokoro
DOX documents remain unchanged intentionally: the production interpreter
precedence, pack ownership, pinned manifests, setup budgets, and verification
workflow did not change. No child boundary or Child DOX Index changed.

## Deviations and unresolved items

- The admitted CI text is treated as untrusted evidence and was corroborated
  against current source; no claim relies on log wording alone.
- The initial worker froze this report before verification. This continuation
  records its actual accepted command and the later canonical interruption;
  the original planned compound command is not counted as executed proof.
- The local focused command cannot substitute for fresh Windows 2025, macOS ARM,
  and macOS Intel exact-SHA jobs. Those hosted reruns remain pending.
- No microphone, speaker, OS permission, pack download, real-model synthesis, or
  human audibility acceptance is exercised.

## Readiness effect

The candidate removes the two evidence-backed foundation blockers without
weakening native path semantics or the real 256 MiB pack ceiling. A passing
focused command establishes current-host regression proof for both corrected
test contracts. Only a fresh complete exact-SHA matrix can establish native
Windows/macOS release readiness; no release or installation claim is made here.
