# Windows voice parity verification — 2026-10-09

## Objective and status

Verify Alex's Windows microphone and Natural Voice screenshots against released
v0.24.11 source `58314f199149c0cb0d21b6f22fb3e482e999d685`. Alex confirms
authentication now works on the tested Windows machine.

**Result: Windows voice parity fails.** Production capture explicitly rejects
Windows; setup and playback contain additional Unix/Linux assumptions. This is
completed source verification, not a repaired implementation or device pass.

## Methods and commands

Used `git show origin/main:<path>`, `git rev-parse`, `rg -n` and bounded `sed`
reads on the released source. Created a clean persistent worktree on branch
`audit/windows-voice-parity-20261009`; Alex's dirty checkout remains untouched.
The previous temporary closeout directory is unavailable in this environment;
its local-only receipts were not assumed present. Read the applicable root,
documentation and foundation DOX chain and the owning voice PRDs.

The [evidence manifest](2026-10-09-windows-voice-parity-verification.json)
retains source hashes, exact finding excerpts and screenshot hashes. Report
content/link/JSON/whitespace checks apply. No Cargo suite, device access, live
provider call, credential access, dependency installation or release occurred.

## Confirmed findings

| Finding | Released source | Consequence |
| --- | --- | --- |
| Windows recorder absent | `apps/agent-vesper-tui/src/voice.rs:522` | `start()` rejects targets other than Linux/macOS. Only `arecord` and `afrecord` follow; the refusal matches the screenshot. |
| Linux conversation player | `apps/agent-vesper-tui/src/voice_conversation.rs:489` | Selects `aplay` on every target; no native Windows/macOS player selection. |
| Linux x64 voice runtime | `crates/vesper-voice-kokoro/src/pack.rs:187` | Installs `onnxruntime-linux-x64-1.28.0.tgz` and `libonnxruntime.so.1.28.0`; extraction/loading share this Linux library, without Windows DLL/macOS dylib selection. |
| Incomplete Windows executable lookup | `voice_readiness.rs:38`; `crates/vesper-voice-kokoro/src/engine.rs:151` | Checks literal PATH filenames such as `espeak-ng`, without trying `espeak-ng.exe`. A present Windows executable can be reported missing; the screenshot does not establish whether one is installed. |
| Unix recognition environment | `apps/agent-vesper-tui/src/voice.rs:491`; `voice_readiness.rs:98` | Uses `bin/python`, without Windows `Scripts/python.exe` preparation/readiness selection. |
| Unix pack default location | `crates/vesper-voice-kokoro/src/pack.rs:147` | Uses `XDG_DATA_HOME`/`HOME`, without a native Windows user-data default; otherwise returns `/nonexistent-agent-vesper-voice-pack`. Actual laptop environment uninspected. |
| Unix capacity probe | `crates/vesper-voice-kokoro/src/setup.rs:785` | Invokes GNU `df -B1 --output=avail` everywhere; ordinary Windows and default macOS need a different probe. Unknown capacity in the screenshot is consistent with this path, not a measured disk fault. |
| Incomplete prerequisite workflow | `apps/agent-vesper-tui/src/settings_host.rs:1560` | Offers Install with missing-espeak wording and no prerequisite setup action there. Readiness directs users to a package manager. Setup's own missing-phonemizer message allows asset placement while blocked, contradicting the confirmation's blanket refusal wording. |

These are local speech implementation gaps, independent of the selected coding
provider. Installing espeak-ng alone cannot provide missing recording/playback
backends or the correct runtime. Removing the refusal message would not fix them.

## PRD and acceptance boundary

The existing push-to-talk PRD preserved the supported recorder set and honest
unsupported-platform reporting. VRO-17 retained OS-tool capture and recorded
cross-platform native capture as a later increment. Those historical scoped
contracts do not satisfy Alex's later Windows/macOS parity request. A general
five-target compile/test matrix can pass while a feature deliberately rejects
one platform. Authentication and RRC tests are not voice acceptance.

Linux voice-device receipts remain valid at their original scope. Full Windows
voice and macOS Natural Voice must not be inferred from them. ACP's documented
audio exclusion remains separate: no new ACP microphone protocol is implied.

## Required repair and unresolved work

Provide platform capture/playback through the provider-neutral speech contracts,
correct executable/interpreter/user-data paths, verified runtime assets per OS
and architecture, and confirmed native Settings prerequisite setup/repair.
Preserve working Linux behavior, bounded audio retention, responsive progress,
Stop/cancellation and explicit Retry/Discard.

Native Windows and both Macs must execute setup, pack verification/synthesis,
recognition, playback, interruption, restart and error acceptance. Actual audio
device/permission results require separate receipts; mocked devices, version/help
checks and green unrelated suites cannot close them. These repairs remain open.

## Files, DOX and readiness effect

Created this report and JSON manifest; linked them from the evidence index and
both owning voice PRDs. Documentation DOX now requires feature-specific platform
evidence. Child indices remain unchanged: no new ownership boundary was added.
Root/apps/crates DOX stays unchanged because this audit changes no production
contract or implementation; their provider-neutral rules still apply.

No program test, Windows/macOS device execution, version bump, push, tag,
installation or release was performed. The verification is complete and the
voice repair remains unimplemented. Report-only changes stay local for the next
authorized implementation commit. Earlier authentication/RRC green receipts do
not establish full Windows/macOS voice parity.

Documentation checks passed for the eight source/hash findings, parsed evidence
JSON, four introduced/report local links, unchanged production source bytes and
`git diff --check`. A whole historical PRD link sweep additionally found the
pre-existing missing `foundation/2026-09-21-agent-progress-0844z.md` in the clean
released checkout; it is retained as a documentation defect, not silently counted
as a successful full-document link check. Screenshots do not display a version,
so their executable identity is unverified; the source findings bind v0.24.11.
