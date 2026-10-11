# Native voice readiness-oracle portability repair — 2026-10-10

## Objective and status

Repair every causal family in the three Release Recovery Controller (RRC)
failures admitted for the macOS Intel, macOS Apple Silicon, and Windows
five-target foundation jobs. Keep the repair confined to stale platform-specific
test expectations and their owning records. Do not change production readiness,
playback, setup, device behavior, release state, remotes, or publication state.

**Status at report freeze:** source and required documentation edits are complete.
Per the bounded repair instruction, focused verification runs only after the final
edit. Its command and result are delivered in the controller response; this report
does not misrepresent that later command as already executed. Fresh exact-SHA
native jobs remain required regardless of the local result.

## Admitted evidence and causal clustering

No prior repair evidence was supplied. The three fingerprints reduce to one
causal family with two stale assertions:

1. `6cfbcb3cc96df8b01d721c18820e10e76e6e9299b42527b698d165c848afa383`
   (`five-target-foundation / macos-intel`):

   ```text
   thread 'pack_success_does_not_imply_device_acceptance' (68677) panicked at apps/agent-vesper-tui/tests/r3_voice_pack.rs:379:5:
   assertion failed: checks.iter().any(|check| check.name.contains("aplay"))
   test pack_success_does_not_imply_device_acceptance ... FAILED
   ```

2. `a6374dea1e3a1dfddfcc270058dd02a86bccd26bbaf296326d766a8a0b523039`
   (`five-target-foundation / macos-apple-silicon`) failed the same assertion:

   ```text
   thread 'pack_success_does_not_imply_device_acceptance' (71710) panicked at apps/agent-vesper-tui/tests/r3_voice_pack.rs:379:5:
   assertion failed: checks.iter().any(|check| check.name.contains("aplay"))
   ```

3. `f96afe0c3a33eb3c3422898a337cb272b108ca57c0a0c44050031878bd960f52`
   (`five-target-foundation / windows-x86_64`):

   ```text
   thread 'settings_panel_and_f9_share_one_assessment' (8044) panicked at apps\agent-vesper-tui\tests\pr4_f9_gate.rs:264:10:
   aplay check present
   ```

Current production source corroborates one cross-platform mismatch rather than
three runtime failures. `voice_readiness_with_interpreter` reports
`native audio backend (device checked on activation)` as ready on both Windows
and macOS, while only Linux resolves `aplay`. `PlaybackOwner::system` likewise
uses owned native WASAPI/CoreAudio on Windows/macOS and the `aplay` child path on
Linux. The macOS pack test still required `aplay` unconditionally. The F9 parity
test exempted only macOS, so Windows fell through to the Linux-only assertion.
Neither failure demonstrates a missing player, broken native backend, or device
acceptance failure.

## Repair hypothesis and changes

**Family — stale Linux-only readiness expectations after native playback
landed:** make both tests branch at the same platform boundary as production.

| File | Causally relevant change |
| --- | --- |
| `apps/agent-vesper-tui/tests/r3_voice_pack.rs` | On Windows/macOS, require the native-audio readiness row, require it to be ready, and preserve the explicit `device checked on activation` limitation. On Linux, continue requiring the `aplay` row. |
| `apps/agent-vesper-tui/tests/pr4_f9_gate.rs` | Apply the existing native-audio/F9-ready branch to Windows as well as macOS. Linux continues to require the missing-`aplay` blocker from both Settings and F9. |
| `apps/agent-vesper-tui/tests/AGENTS.md` | Record the durable platform-specific oracle contract for both suites. |

The repair does not weaken the tests into generic success checks. It asserts the
actual platform row and retains the distinction between static backend readiness
and device acceptance during activation.

## Methods and files

Inspected:

- root, app, TUI, TUI-test, documentation, and foundation `AGENTS.md` contracts;
- the two failing integration tests;
- `apps/agent-vesper-tui/src/voice_readiness.rs`;
- `apps/agent-vesper-tui/src/voice_playback.rs`;
- `apps/agent-vesper-tui/Cargo.toml` feature relationships;
- the existing 2026-10-10 native voice repair and evidence indexes.

Changed source/test contracts:

- `apps/agent-vesper-tui/tests/r3_voice_pack.rs`
- `apps/agent-vesper-tui/tests/pr4_f9_gate.rs`
- `apps/agent-vesper-tui/tests/AGENTS.md`

Changed records:

- `docs/foundation/2026-10-10-native-voice-readiness-oracle-portability-repair.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/AGENTS.md`
- `docs/AGENTS.md`
- `docs/Agent_Vesper_Release_Recovery_Controller_PRD.md`
- `docs/voice-oracle-extraction-prd.md`

## Focused verification

Required post-edit command, selecting exactly the two failed tests:

```text
cargo test -p agent-vesper-tui --features voice-kokoro --test r3_voice_pack pack_success_does_not_imply_device_acceptance -- --exact && cargo test -p agent-vesper-tui --features voice-kokoro --test pr4_f9_gate settings_panel_and_f9_share_one_assessment -- --exact
```

At this report's required pre-verification freeze, the command is **NOT YET
EXECUTED**. Its actual exit status and verbatim test totals must be taken only
from the subsequent tool receipt and delivery summary. No broader suite is
claimed.

## DOX pass

The nearest test DOX now states the platform split the tests enforce.
`docs/foundation/AGENTS.md` and `docs/AGENTS.md` own this new report, the
foundation evidence index links it, and both the RRC and VRO-17 PRDs link the
bounded recovery evidence. The parent app and TUI DOX remain unchanged
intentionally: no application responsibility, production behavior, child index,
or host-parity exception changed. Root DOX remains unchanged for the same
reason.

## Deviations and unresolved items

- The admitted log text was treated as untrusted and corroborated against current
  source before editing.
- No red rerun was performed because the admitted exact-SHA failures already
  identify both stale assertion sites and the controller instruction permits only
  one final focused command after all edits.
- The report is intentionally frozen before that command; it preserves the
  verification as unexecuted rather than inventing a receipt.
- No program-wide, Clippy, format, documentation-link, MSRV, cross-target, device,
  or live-audio gate is run in this bounded repair.
- Local Linux execution can prove compilation and the retained Linux branches,
  but it cannot execute `cfg(windows)` or `cfg(target_os = "macos")`. Fresh
  Windows 2025, macOS Intel, and macOS Apple Silicon exact-SHA jobs remain open.
- No microphone, speaker, OS permission prompt, pack download, real-model
  synthesis, or human audibility acceptance is performed.

## Readiness effect

The candidate aligns both stale integration-test oracles with the already-shipped
production playback split while retaining explicit device-acceptance limits. A
successful focused command is sufficient local regression proof for the two
edited test targets, not native release proof. Only a fresh complete exact-SHA
matrix can clear the admitted native lanes; no release, installation, or hardware
acceptance claim is made.
