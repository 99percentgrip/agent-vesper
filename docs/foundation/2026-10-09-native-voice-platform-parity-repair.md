# Native voice platform parity repair — 2026-10-09

## Objective and status

Implement Alex's approved Windows/macOS voice parity repair, preserve working
Linux behavior and provider neutrality, verify named feature paths, then make
one candidate release through the native Release Recovery Controller (RRC).

**Local implementation verified; native exact-source CI and hardware acceptance pending.**
The earlier [released-source audit](2026-10-09-windows-voice-parity-verification.md)
remains historical failure evidence for v0.24.11
`58314f199149c0cb0d21b6f22fb3e482e999d685`. Authentication works by Alex's report;
it is not voice acceptance. Work stays on isolated branch
`audit/windows-voice-parity-20261009`; Alex's dirty checkout and installation
remain untouched.

**Exact-CI follow-up:** native RRC subsequently pushed the v0.24.12 candidate,
whose fully settled matrix exposed a shared-reader compile regression on Windows
and both macOS targets. Its failed repair dispatch also exposed a journal
recovery defect. The [corrective execution report](2026-10-09-native-voice-ci-and-rrc-recovery.md)
retains those failures and owns the canonical descendant correction. This report's
earlier local/audio-module successes do not certify that failed native candidate.

## Changes and traceability

| Confirmed gap | Owning repair | Required proof |
| --- | --- | --- |
| Windows capture rejected; macOS external recorder | TUI `voice_native_audio.rs` owns WASAPI/CoreAudio streams; bounded downmix/rate conversion and managed WAV writer | Native PCM callback/storage tests; separate device/permission test |
| Linux player selected everywhere | `PlaybackOwner::system` uses native Windows/macOS stream ownership, bounded queue, Stop and playback-timestamp drain | Native queue/receipt tests and device playback |
| Linux x64 runtime on every OS | Kokoro `pack.rs`, `engine.rs`, `setup.rs` select fixed native archives/libraries for five targets | Exact download/member digests and real native synthesis |
| Missing Windows executable suffixes | Native `.exe` discovery and default espeak-ng installation directories; typed paths allow spaces | Fixture suffix/space tests; real phonemizer probe |
| Unix-only recognition interpreter | Native `Scripts/python.exe` / `bin/python`; private setup and healthy environment reuse | Native dependency setup/import; separate microphone transcription |
| Unix-only pack location | Windows LOCALAPPDATA and macOS Application Support, preserving existing Mac legacy data; absolute XDG isolation | Native path cases and actual isolated installation |
| GNU capacity probe on Windows/Mac | `fs2` destination/nearest-existing-ancestor capacity; unknown/insufficient capacity refuses transfer | Capacity regression and native plan/setup |
| Missing pronunciation prerequisite left users blocked | Native Settings confirmed setup/repair with OS approval, progress, bounded cancellation and readiness probes | Native dependency probe; separate real-user OS authorization |

Additional repairs: fail-closed private user-data roots, Settings draft retention
after setup failure/cancellation, canonical WAV headers, finite/saturated signal conversion,
output rate/channel bounds, partial-audio retention, native capture failure/cap
projection, Windows C++ runtime prerequisite detection, menu index correction,
legacy Mac venv reuse and removal of stale Linux/model Details copy. Voice
setup does not save provider/voice choices or expose a model installation tool.
ACP's existing no-audio protocol boundary remains explicit.

## Primary sources and artifact identity

The [measured manifest](2026-10-09-native-voice-platform-parity-repair-source.json)
records exact archive/member sizes and SHA-256 from official downloads:
[CPAL 0.17.3](https://docs.rs/cpal/0.17.3/cpal/),
[ONNX Runtime 1.28.0](https://github.com/microsoft/onnxruntime/releases/tag/v1.28.0),
[Intel Mac runtime 1.23.2](https://github.com/microsoft/onnxruntime/releases/tag/v1.23.2),
[uv 0.12.24](https://github.com/astral-sh/uv/releases/tag/0.12.24),
[espeak-ng 1.52.0](https://github.com/espeak-ng/espeak-ng/releases/tag/1.52.0),
and [Microsoft's native runtime](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist).
ORT's current artifacts lack Intel macOS; the selected official Intel archive
uses the adapter's compatible C API v17. This is an explicit runtime-version
distinction requiring real-model proof, not a same-version claim.
[Homebrew 4.6.17's package workflow](https://github.com/Homebrew/brew/blob/4.6.17/.github/workflows/pkg-installer.yml)
provides the verified dual-architecture installer when no healthy brew exists.

No runtime/model is vendored into Vesper. Optional downloads require confirmation
and integrity checks. Runtime licenses/notices retain exact platform bytes.
Private Python preparation is delegated to verified or already healthy uv;
package imports and pronunciation are probed before readiness.

## Methods and current evidence

- Root, app, crate, tests, workflow and documentation DOX chains read before edits.
- `cargo xtask verify`: passed full workspace formatting, strict Clippy, all-feature
  tests, architecture (31 packages), fixture checks and **159 exact acceptance
  cases**. These tests use offline providers; live-model effectiveness is not measured.
- Full locked Rust 1.88 workspace all-feature tests passed in a separate target.
- Final source focused all-target/all-feature tests passed: TUI library 312,
  binary 177 (one ignored), harness 415 (six ignored), voice 86, Kokoro 44,
  plus integration cases. Ignored checks are not acceptance evidence.
- Final strict workspace Clippy and formatting passed. Latest Rust 1.88 TUI
  delta verification is separately recorded in the receipt manifest.
- Windows, Intel Mac and ARM Mac production audio/storage/PCM/player modules and
  their test source compile with warnings denied in an isolated compile harness.
  These are module compile receipts, not full application or hardware execution.
- A full Windows cross-check on Linux failed in `aws-lc-sys` because the native C
  toolchain/Windows SDK is absent. No Windows application success is claimed locally.
- Fresh Linux production pack setup in private `/tmp` XDG storage passed every
  digest and silent synthesis check; retained manifest bytes: 118,004,068.
  Actual output: Michael 53,200 samples, peak 16,148, RMS 1,796.638;
  Heart 45,600 samples, peak 15,833, RMS 2,035.433. No audio device was opened.
- Native Settings consent/decline/draft/discard PTY passed against the rebuilt TUI.
- Offline CI guards: three passed; workflow YAML parsed with five native families.
- Cargo deny (advisories, bans, licenses, sources) and cargo audit passed.
- Earlier canonical test failed its stale manual-install expectation; corrected
  Settings remedy passed. Earlier consent fixture failed its stale label; corrected
  fixture passed. Neither failed run is represented as success.
- An initial MSRV run sharing canonical artifacts was intentionally interrupted
  before restarting in an isolated target; interruption is not a source failure.
- Native exact-SHA CI, publication and real hardware remain pending.

The [receipt manifest](2026-10-09-native-voice-platform-parity-repair-receipts.json)
binds command logs and source hashes to the
[compressed receipt archive](2026-10-09-native-voice-platform-parity-repair-receipts.tar.gz).

Commands use bounded Cargo concurrency and no incremental/full debug artifacts.
Real setup/signal examples use private test storage and an existing phonemizer;
they do not install an application, call a provider or access real credentials.
The five-target workflow additionally prepares disposable runner prerequisites,
isolates application state, and requires actual production dependency readiness,
pack verification and both signal markers; missing receipts fail the lane.

## Files and DOX pass

Changes cover TUI manifest, platform/PCM/audio owners, dictation/playback/preview,
readiness/Settings and recognition paths; harness native voice dependency setup
and guarded acceptance example; voice subprocess path validation and regressions;
Kokoro runtime manifest/loading/setup; native CI driver/guards/workflow; Cargo lock;
installation guide, owning voice PRDs, this report/source manifest and evidence index.
Nearest owning DOX documents are updated in the same candidate. Root/apps/crates
parent structure and child indexes remain unchanged: no new ownership subtree,
provider or cross-host audio capability is introduced. The user guide's generic
F5/F9 and ACP boundary stay applicable.

## Unresolved acceptance and readiness effect

| Boundary | Current status |
| --- | --- |
| Local canonical/MSRV/acceptance and focused final source | Passed; see receipt manifest |
| Native five-target application/setup/signal | New prerequisite gate; pending exact-source execution |
| Windows/Mac microphone recording, real transcription and speaker listening | Unexecuted; real hardware required |
| Real-user OS approval/reboot paths | Unexecuted; headless runner package preparation is not approval UX proof |
| Original Windows laptop / both Mac families | No new device receipt |
| One bump, exact-SHA RRC CI/tag/publication/Registry/final delivery | Pending; no manual release bypass |

Compile success, nonzero PCM and Linux device history cannot close the remaining
hardware rows. Preserve failed, ignored and unexecuted checks. Publication does
not establish universal voice parity. Alex retains local update/device testing.
