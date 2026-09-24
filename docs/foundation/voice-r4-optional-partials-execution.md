# VRO-17 R4 — Optional-Partials Amendment Execution

Work unit: 2026-09-23 (evening, second unit). Baseline `main` @ `8f258ba`
+ the dirty VRO-17 voice tree. Alex approved **Option B** from
`voice-r4-partials-scope-decision.md`: live partial transcripts become an
**optional STT-provider capability**, not a mandatory v1 requirement.

## Amendment applied

**Old R4:** live partial transcripts stream during capture when enabled
through the declared partial mode — a mandatory production capability.

**Amended R4:** Live partial transcripts are an optional STT capability.
Each STT provider declares `BufferedRepass`, `Incremental`, or no partial
support. When enabled **and** supported, partials are display-only,
best-effort, bounded, cancellable/stale-safe, never submitted as turns,
and always superseded by the final transcript. **A final-only provider
remains fully conformant with VRO-17.** No provider is required to emulate
partials via expensive repeated full inference.

## Current provider capability table (source-verified, unchanged)

| STT backend | Declared partials | Notes |
|---|---|---|
| CPU Whisper sidecar (`SidecarStt`/`SharedSidecarStt`) | `None` (final-only) | `stt_sidecar.rs:235` |
| Self-hosted HTTP (`HttpStt`) | `None` (final-only) | `stt_http.rs:149`; pinned-worker repass exists upstream but is not wired (D21) |
| FLM NPU (`FlmNpuStt`) | `None` (final-only) | `voice_flm.rs:687` + module contract; endpoint has one final response |

**All current production STT adapters are final-only.** No fake FLM
partials, no repass, no synthetic partials were introduced.

## Settings capability behavior (before → after)

**Before:** the Voice panel rendered
"Live partial transcripts while capturing · current ON/OFF" — with every
current backend final-only and `scope.partials` consumed by no runtime
code, Alex's saved `partials = true` presented a usable-looking toggle
that could never do anything (dormant + misleading).

**After:** the panel renders `voice_accel::partials_settings_row(scope)`:

- Final-only backend (every current one, including refused/unavailable
  selections): **"Partial transcripts · unavailable for the selected
  speech-recognition backend"** — truthful, never flippable (the toggle
  handler is capability-gated and refuses to flip when unsupported).
- A future capable backend: "Partial transcripts · current ON/OFF
  (live repass | incremental)" with its declared mode.
- The saved preference is **never destroyed**: gating is
  presentation/interaction only; round-trip persistence is pinned by
  test. No new config store, no provider boolean — the descriptor-derived
  capability (`selected_stt_partials_mode`, from the same policy
  resolution as the F9 gate, no adapter construction) is the authority.

## Reasoning-provider neutrality — production path (re-verified)

F9 capture → selected `VoiceStt` → final transcript →
`dispatch_voice_submission` → `spawn_submitted_prompt` (the identical
typed-Enter seam) → `AgentLoop`/`ProviderRegistry` → selected reasoning
provider → provider-neutral `ContentDelta`/settlement events →
`HostEvent::AgentText` → hygiene → selected `VoiceTts` → `PlaybackOwner`.
Zero reasoning-provider names in voice modules (sweep repeated); the two
`GLM_*` hits remain speech-env naming legacy. **Adding a conforming
reasoning provider requires voice-specific production changes: NO.**

## Two-provider regression proof (new invariant guard)

`apps/agent-vesper-tui/tests/voice_provider_neutrality.rs` (8 tests,
`required-features = ["voice-conversation"]`): two distinct conforming
fake reasoning providers (`fake-alpha`, `fake-beta`) through the normal
registry-shaped composition; typed input reaches either provider through
the generic path; F9-origin voice submission reaches either identically
(ordinary-string prompts, no envelope); assistant text from both returns
through the same provider-neutral event translation into the same
synthesis boundary; a four-way (origin × provider) parity sweep asserts
exactly one dispatch per submit with 2/2 per provider. **Structural
coupling detection:** any future `if provider == "x"` voice branch starves
the other fake — the symmetry fails loudly. No credentials, no network.
Classified as a **new invariant guard** (the architecture was already
correct; nothing failed pre-fix on this axis).

## Red-first evidence

The Settings test pins the **production** row function
(`partials_settings_row` — extracted so panel and test share one source).
Reverting that function to the old always-ON/OFF behavior:
`final_only_cpu_scope_shows_partials_unavailable` **FAILED** (assertion:
"a final-only backend must never present the toggle as enabled");
restored, the suite is green ×4. The first red attempt that mirrored the
row inside the test was vacuous against production — caught and fixed by
extracting the production fn (recorded honestly).

## Changed files

- `apps/agent-vesper-tui/src/voice_accel.rs` —
  `selected_stt_partials_mode` (capability authority, descriptor-only) +
  `partials_settings_row` (production row text).
- `apps/agent-vesper-tui/src/settings_host.rs` — the panel renders the
  production row; the partials toggle is capability-gated.
- `apps/agent-vesper-tui/tests/voice_provider_neutrality.rs` — new (8
  tests: 4 capability presentation, 4 neutrality).
- `apps/agent-vesper-tui/Cargo.toml` — test target registered.
- PRD / audit / evidence-index / migration-status / DOX (below).

## Commands and results

`voice_provider_neutrality` 8/8 ×4 (RED proven on the reverted
production fn, GREEN restored) · feature test sweep 16 suites ok ·
PartialGate suite 30/30 (pr1_adapters, unchanged) · `voice_policy_parity`
7/7 · `pr4_wiring`/descriptor suites green · clippy `-D warnings` clean
(lib voice-conversation, bin voice-flm/kokoro, tests) · fmt applied ·
architecture 30 pkgs · naming-guard clean · **acceptance 23/23** (R4
status change) · PTY on the candidate bytes: `voice_pty` PASS (F5),
`r3_loop` PASS (F9 CPU), `flm_f9_loop` PASS (F9 NPU; CPU recognizer
untouched; no flm leak after cleanup).

## Candidate

`target/voice-candidates/agent-vesper-tui-r4-optional-partials` — SHA-256
`673f9fb8f82d4c5f5ec2ac42…` (byte-identical to release, features
`voice-flm,voice-kokoro`), source `8f258ba` + dirty tree. Prior
candidates preserved; nothing installed.

## Why no accepted user capability was removed

Partials never rendered anywhere (no interim UI state existed) and no
runtime code consumed `scope.partials`; the accepted CPU/NPU experiences
never produced or displayed a partial. The change converts a misleading
dormant toggle into truthful unavailability and preserves the preference.
`PartialGate` remains bounded, stale-safe, reusable infrastructure
(library tests retained) for any future capable adapter.

## R4 verdict

**PASS.** Amended optional-capability contract applied consistently;
final-only providers explicitly conformant; Settings truthful;
PartialGate retained as infrastructure; no fake FLM partials; final
submission behavior unchanged; provider-neutrality path intact with the
two-provider guard green; all applicable gates pass.

## Remaining VRO-17 open items (re-derived from the amended PRD)

- **R6** — Alex-operated interruption/recovery device acceptance.
- **PR-5** — ACP voice decision/exclusion documentation + cross-host
  parity assertion; commit the voice tree; exact-commit
  canonical/MSRV/five-target/web-driver/release workflows; user docs;
  tag.

(R2/R3 cloud-optional, R16b capability-gated, R16a/R20/R4 closed at their
recorded scopes.)
