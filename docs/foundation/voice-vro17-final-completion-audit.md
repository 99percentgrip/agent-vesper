# VRO-17 Final Completion Audit

Audit date: 2026-09-23 (after the NPU user acceptance closeout);
**amended the same day** to incorporate Alex's three approved scope
amendments (R2 cloud-STT optional; R3 cloud-TTS optional; R16b NPU TTS
capability-gated with CPU first-class). Auditor: Agent Vesper,
evidence-led, no implementation. The §B matrix, §H decision and counts
below reflect the AMENDED contracts; pre-amendment verdicts are noted
where they changed.

**Current closeout update (2026-09-24):** the §2.4 production binding was
repaired and Alex subsequently passed the real-device R6 matrix. Alex also
accepted the latest short-reply quality candidate as “Looks like smooth,” and
the pronunciation repair remains accepted on the tested path. The current
ledger therefore closes R1–R20 at their approved scopes. PR-5 is now in its
host/release unit: ACP's audio/control exclusion and provider/runtime parity are
evidenced, user/cloud-roadmap docs are reconciled, and the release feature
matrix includes Kokoro/FLM. VRO-17 remains open until exact-commit gates and
publication complete.

Baseline: branch `main` @ `8f258ba` ("fix(mcp): retain conversation-owned
stdio sessions across calls") **+ 159 dirty/untracked paths** — the entire
post-v0.23.3 VRO-17 voice surface (63 voice-related paths) is uncommitted
working-tree state. No commit, tag, or release contains it. No files were
modified by this audit except this report and the discoverability indexes.

Compiled feature surfaces audited: `vesper-voice` (pure core + `stt-sidecar`,
`stt-http`, `tts-subprocess`), `vesper-voice-kokoro` (`ort`), TUI
(`voice-conversation`, `voice-kokoro`, `voice-flm`). Bare-core and default
TUI builds compile clean (verified this audit). ACP contains **zero voice
code** (grep over `apps/agent-vesper-acp/src`: no voice surface; its
AGENTS.md documents push-to-talk voice as an interactive-terminal exclusion).

---

## §A Current-state facts (source-verified)

- **Registered STT adapters (production reachability):** `SharedSidecarStt`
  (CPU conversation + dictation, `voice_shared_stt.rs`) and `FlmNpuStt`
  (NPU, `voice-flm` builds only, constructed by `SelectedStt::build()` from
  the saved policy after real in-process verification). Library adapters
  `SidecarStt`/`HttpStt` exist; `FailoverStt` exists; **no host constructs
  `FailoverStt` or `PartialGate` in production** (grep: zero construction
  sites outside tests). **No cloud STT adapter exists anywhere —
  correct under the 2026-09-23 amendment (optional gated integration).**
- **Registered TTS adapters:** `KokoroTts` (vesper-voice-kokoro; used by
  conversation/Preview via `SpeechWorker`) and `SubprocessTts`
  (synthesis-only system-engine baseline; not selected by the TUI
  conversation path). **No cloud TTS adapter exists — correct under
  the 2026-09-23 amendment (optional gated integration).**
- **Acceleration route registry** (`voice_accel.rs`): exactly one route —
  FLM NPU STT — registered only in `voice-flm` builds; honest empty
  registry otherwise. No TTS route ever.
- **Settings entries:** Voice panel (enable/dictation+conversation,
  engine/voice, per-stage compute CPU/Automatic/strict-NPU, FLM Verify,
  pack management) — TUI only.
- **Egress classes in code:** `OnDevice` (sidecar, FLM, Kokoro),
  `SelfHostedRemote` (HttpStt, unconstructed). `ThirdPartyCloud` exists as
  a type + config-validation classes only; **no adapter can produce it**.
- **ACP voice surface:** none. No ACP voice tests, no ACP voice parity, no
  documented per-control exclusions beyond the one AGENTS.md line.

## §B R1–R20 requirement matrix

Verdicts use only the allowed values. "Automated" = test receipt on the
current dirty tree (all rerun green this audit or this session, see §E).

| Req | Current contract | Production owner | Reachability | Automated evidence | Real-device/real-model evidence | Verdict | Remaining exact gap |
|---|---|---|---|---|---|---|---|
| **R1** agent-free core | No reasoning/tools in `vesper-voice`; arch allowlist (`vesper-domain`, `vesper-security`) | `crates/vesper-voice` | Live: hosts compose; core never dispatches | `cargo xtask architecture` 30 packages (rerun green this audit); PR-0 dep tests | n/a (architectural) | **PASS** | none |
| **R2** STT interchangeable, ≥1 production-capable **local** route *(amended 2026-09-23: cloud = optional gated integration)* | v1 ships ≥1 production-capable local STT; architecture/config/credentials/failover/egress must *permit* cloud adapters; none supported until a real adapter passes its gate | `SidecarStt`+`SharedSidecarStt` (local, production-constructed), `FlmNpuStt` (local NPU), `HttpStt` (self-hosted, library) | Local: reachable in production (CPU + NPU both user-exercised). Cloud: permitted-by-architecture, none implemented — correct per amendment | PR-1 suite 30/30 (rerun green); `voice_flm_route` 13/13; F5 `voice_pty` PASS | CPU dictation + NPU conversation user-accepted | **PASS** *(amended)* | Optional follow-on: a concrete cloud adapter behind its own auth/transport/privacy/fixtures/acceptance gate |
| **R3** TTS interchangeable, ≥1 production-capable **local** route *(amended 2026-09-23: cloud = optional gated integration)* | v1 ships ≥1 production-capable local TTS; future cloud TTS needs auth/transport/privacy/egress/mandatory pre-cloud redaction (R7)/fixtures/acceptance | `KokoroTts` (local neural, selected), `SubprocessTts` (local baseline) | Local: reachable and user-accepted. Cloud: permitted-by-architecture, none implemented — correct per amendment | Kokoro suite (86/13/13); PR-2 suite 19 | Kokoro listening accepted; pronunciation repair accepted on the tested path with no new regression in the latest smoothness retest | **PASS** *(amended)* | Optional follow-on: a concrete cloud adapter behind its own gate |
| **R4** live partial transcripts *(amended 2026-09-23, Alex-approved Option B: optional STT capability; final-only providers conformant)* | Providers declare the mode; partials display-only/bounded/stale-safe/never-submitted when enabled AND supported | Capability authority `voice_accel::selected_stt_partials_mode` (descriptor-only, no adapter construction); capability-aware Settings row + guarded toggle; `PartialGate` retained as reusable infrastructure | All three production adapters final-only (`None`) — Settings shows truthful unavailability; no runtime partial wiring (correct: nothing can produce one) | `voice_provider_neutrality` 8/8 (capability presentation + two-fake-provider neutrality guard; RED proven on the reverted production row fn); PR-1 PartialGate suite 30/30 unchanged | n/a (partials never rendered in any accepted experience; no capability removed) | **PASS** *(closed 2026-09-23, `voice-r4-optional-partials-execution.md`; previously IMPLEMENTED — acceptance open)* | none binding (a future capable adapter opts into the existing contract) |
| **R5** sentence-gated streaming TTS + per-turn first-audio latency | TTS streams while agent streams; latency measured per turn | `SpeechWorker` (bounded first piece, sentence-level successors, **depth-2 bank**) + PR-2 hygiene; stage footer shows live stage+elapsed | Live: the whole accepted conversation pipeline | Pipeline fixture red→green (bank); 4 PTY modes + FLM PTY; short-reply full-path receipt 0.972 s → 0.200 s | Earlier listening preserved; latest voice-quality candidate `f3b736f6…` user-accepted as “Looks like smooth” on the tested setup | **PASS — scoped limitation** | Per-turn first-audio telemetry exists as stage footer (`stage_status`, `VoiceTurnReport.stage_ms` in core) but no persisted per-turn numeric latency record; no numeric acoustic threshold was set or inferred |
| **R6** barge-in/Stop | Stop cancels speech + transactional runtime cancel; bounded acked-playback interruption note; no tool replay; **BargeIn = stop + cancel + immediate new capture (§2.4 binding)** | `ConversationController`/`VoiceSession` + the production TUI F9/Stop bindings | One Speaking+F9 routes through genuine BargeIn: playback/synthesis stop, normal runtime cancellation request, immediate replacement capture; explicit Stop cancels without capture | PR-3 race matrix; `voice_r6_binding` production-entry proof; repeated-cycle, provider-neutrality and stale-generation regressions | **PASS:** one-press and repeated barge-in, old speech stop/no resume, exactly one replacement turn, explicit Stop with no accidental capture, and later F9 recovery user-confirmed on the tested setup | **PASS** *(closed 2026-09-24)* | none at approved scope; no every-platform/provider, numeric latency, interruption-note audibility, or tool-replay claim is inferred from the device run |
| **R7** secret redaction pre-cloud | Non-disableable redaction before third-party cloud synthesis; canaries | PR-2 hygiene engine + PR-0 egress validator | Engine reachable (feeds conversation synthesis); **cloud path unexercised (no cloud TTS exists)** | PR-2 canary fixtures (16 hygiene tests) green | n/a | **PASS — scoped limitation** | Exercised against local synthesis only; the cloud-synthesis obligation activates with any cloud TTS (R3) |
| **R8** exactly one terminal report | One `VoiceTurnReport` per accepted turn; absent stages absent | PR-3 session emits; TUI consumes `TurnDone(report)` | Live in conversation path | PR-3 uniqueness tests; TUI settlement preservation tests | n/a (structural) | **PASS** | none |
| **R9** unconfigured parity | Deterministic surfaces identical; no startup side effects unconfigured | TUI: parity tests. **ACP: no voice code at all** (trivially identical) | TUI verified; ACP structurally identical (no surface) | `feature_enabled_unconfigured_startup_constructs_nothing`; default-build suite (395/0 at PR-4; default suite green this audit) | n/a | **PASS** (TUI); ACP parity is vacuous until PR-5 decides surface/exclusion | PR-5 decision documents the ACP exclusion (or adds a surface) |
| **R10** dictation preserved | F5 lifecycle/VAD/acceptance preserved through re-host | `voice.rs` worker retained (re-host decision = retention) | Live | `voice_pty.py` full lifecycle PASS (rerun this session: mouse/F5, 10-min PCM, >90 s progressive, retry/discard, disk-failure, shutdown) | F5 historically user-exercised; VAD behavior pinned | **PASS** | none (retention path); the alternate re-host is not required |
| **R11** truthful absence/silence/inference | `NoSpeech` vs `Unavailable` vs `Inference`; no exception→silence | Adapters + D21 provenance; FLM VAD composition (silence → `VadConfirmedSilence`, zero ASR requests; VAD failure = error) | Live both CPU and NPU paths | `voice_flm_route` 13/13 incl. silence/empty/failure matrix (rerun ×4 green); `voice_vad_worker.py` PASS | Real-model receipt: silence 0.20–0.35 s confirmed silence; speech surrogate transcribed | **PASS** | none |
| **R12** privacy/no content in telemetry | Metadata allowlist; no audio/transcript in logs/errors/timing | Report design + adapters | Live; audit of NPU-era diagnostics: FLM logs classify *allowlisted* vendor lines only; child stderr file is vendor-owned temp, zero bytes in practice; `record_last_stt_route` stores route labels, not text | Redaction fixtures; PR-1/PR-2 audits | n/a | **PASS** | none found; keep the allowlist discipline for future diagnostics |
| **R13** manual capture only | User-initiated; no wake word/always-on/auto-turn-taking | F5/F9 push-to-talk | Live; **no wake-word code exists** (grep) | Lifecycle suites | n/a | **PASS** | none |
| **R14** egress policy-gated | On-device never spills to remote; cross-class failover needs config; failures never change policy | PR-0 validator + descriptors | Live for existing classes; **no cloud adapter exists to spill** | Config validation tests; FLM/sidecar/Kokoro descriptors all `OnDevice`; `HttpStt` `SelfHostedRemote` with redirect refusal | n/a | **PASS — scoped limitation** | Real cross-class failover is unexercisable until ≥2 egress classes are actually selectable in production (tied to R2/R3 cloud decisions) |
| **R15** playback evidence per segment | Ack-based; `Unknown` honest; heard-ness never from synthesis completion | `PlaybackOwner` receipts → session validation | Live | PR-3 receipt-validation tests; playback-diagnostics suite; the pause-recording footer decoded to exact stages | Automated only | **PASS** (contract + enforcement) | Alex-audibility remains user-confirmed only via listening records, which is the correct evidence class |
| **R16a** NPU STT | Conditional per machine/backend; verified-only registration; CPU stays usable; selected-vs-actual truthful | `voice_flm.rs` + `voice_accel.rs` + Settings Verify | Live: `SelectedStt::build()` routes F9 through FLM when verified; CPU blind otherwise | `voice_flm_route` 13/13; `voice_execution_policy` 12; FLM PTY end-to-end ×3 (Settings save → Verify → F9 → adapter → one turn); Verify-reliability regression (red→green) | Real-model receipts (silence/speech/warm); **2026-09-23 user acceptance** ("working with npu", perceived natural/continuous); placement = process/device/model correlation only | **PASS — scoped limitation** | Quantitative natural-speech accuracy fixtures; per-request offload receipts (endpoint has none — permanent scope note, not a defect); repeated-interruption NPU device cases; tested-artifact identity pending |
| **R16b** NPU TTS *(amended 2026-09-23: optional / capability-gated, never an unconditional v1 blocker; CPU TTS first-class)* | If a verified compatible backend/model path exists it may be added after backend/model/inference/offload/quality/latency acceptance; without one, VRO-17 is **conformant on CPU**; CPU Kokoro is never relabeled NPU-backed | none (no route registered; honest absence) | CPU synthesis reachable and user-accepted; no NPU TTS route exists or is advertised | `voice_execution_policy` independent-stage resolution; empty-TTS-route registry tests | CPU Kokoro accepted in every listening record; the 2026-09-23 "npu" wording covers recognition only | **OPTIONAL / CAPABILITY-GATED — CONFORMANT ON CPU** *(amended; previously OPEN)* | None binding. Any future route needs the full acceptance chain first |
| **R16c** CPU usability | Ordinary CPU voice on non-NPU machines, no install/warnings | Default builds + empty registry | Live | `voice_execution_policy` no-NPU matrix (zero accelerator calls; Automatic→CPU ordinary; strict refusal names stage) | CPU conversation user-accepted historically | **PASS** | none |
| **R17** zero production audio files | No files/cache from synthesis | Kokoro in-memory PCM; bank is in-memory (verified: no file writes in `voice_speech_worker.rs`) | Live | PR-2 storage receipts; no-file proofs | n/a | **PASS** (re-checked after the depth-2 bank: bank adds memory, not storage) | none |
| **R18** probe audio bounds | ≤8 MiB/file, ≤32 MiB aggregate, cleaned | Probe examples only | n/a in production | PR-2 receipts (334 KiB peak, 0 residual) | n/a | **PASS** | none |
| **R19** disk-write reserve | Refuse on unknown/low free space; never switch destination | Capture store + any optional write | Live (capture store defers) | Contract tests | n/a | **PASS** | none |
| **R20** capture storage caps | Caps + aggregate accounting + cleanup + crash recovery on **shipped dictation** | ONE managed store for every explicit capture — default F5 and feature F9 alike (feature builds' earlier `self.managed` was dead state; the recorder always wrote a plain tempdir) | Live in both build flavors (PTY-proven) | `voice_r20_default_capture` 11/11 (every feature set); store suite; `voice_pty`/`r3_loop`/`flm_f9_loop` PTY PASS on the repaired candidate | n/a (storage safety; audible behavior unchanged) | **PASS** *(closed 2026-09-23, `voice-r20-default-capture-repair.md`; previously IMPLEMENTED/OPEN-split)* | none |

## §C Goals and Non-Goals

| Goal | Current satisfaction |
|---|---|
| 1. Real-time bidirectional voice on the existing runtime | **Satisfied for the tested TUI conversation experience**; R6 interruption/recovery is user-accepted on the tested setup. No universal or numeric latency claim |
| 2. Interchangeable STT, ≥1 local + ≥1 cloud | **Satisfied at amended scope:** production local routes exist; a concrete cloud adapter is optional and gated (R2 PASS) |
| 3. Interchangeable TTS, ≥1 local + ≥1 cloud | **Satisfied at amended scope:** Kokoro is the accepted local route; a concrete cloud adapter is optional and gated (R3 PASS) |
| 4. Barge-in | **Satisfied on the tested setup:** one-press/repeated interruption, explicit Stop and later-F9 recovery accepted (R6 PASS) |
| 5. Live partials | **Closed at amended scope:** optional STT capability; current final-only providers conform and Settings states that truthfully (R4 PASS) |
| 6. Deterministic hygiene + cloud redaction | Engine complete + reachable; cloud path dormant (R7 scoped) |
| 7. Per-stage latency telemetry | Stage footer + core report fields live; no persisted per-turn numeric record (R5 scoped) |
| 8. Dictation preserved/re-hosted | **Satisfied via retention** (F5 unchanged, suites green) — the re-host itself was a decision, resolved as retention |

Non-Goals re-verified in source: no wake word/always-on (R13 PASS), no
LAN/browser HUD/telephony code, no agent-in-voice (R1 arch gate), local
paths first-class (CPU default everywhere). **All non-goals remain
non-goals.**

## §D Phase gates

| Phase | Planned gate | Current implementation | Current evidence | Verdict |
|---|---|---|---|---|
| PR-0 | Contracts/rails | Complete (pure core, arch rails) | 61 crate tests; arch gate rerun green (30 pkgs) | **PASS** |
| PR-1 | STT adapters/failover/partials | Sidecar+HTTP+Failover+PartialGate complete | 30/30 rerun green ×4; real-model probes recorded | **PASS** |
| PR-2 | TTS/hygiene/storage | Hygiene + subprocess TTS + bounds complete | 19+16 tests; storage receipts | **PASS** (R3-cloud portion tracked separately) |
| PR-3 | Session/barge-in | Complete | 29 tests; reducer in production use | **PASS** |
| PR-4 | TUI/device integration | Feature + wiring + repairs complete; R20, R4 and R6 subsequently closed | CPU/NPU production loops, listening records, R6 binding repair and real-device acceptance | **PASS — approved scope** |
| PR-5 | ACP + release | ACP `audio=false` exclusion documented and real-process pinned; generic provider/runtime and cancellation parity retained; user docs/cloud roadmap reconciled; release workflow now compiles TUI Kokoro/FLM features | `voice-pr5-host-release-execution.md`; ACP process test; two-provider neutrality test | **IN PROGRESS — exact-commit gates/publication open** |

Remaining PR-5 sequence: commit the isolated VRO-17/v0.23.4 tree, run the
local and remote exact-commit gates, tag only after the four required push
workflows pass, publish and verify assets/checksums, update the continuous
registry PR, then record the final release receipts.

## §E External/release gates

| Gate | Current-tree evidence | Historical | Exact-commit | Verdict |
|---|---|---|---|---|
| Workspace all-features | **2838 passed / 0 failed, 187 suites (rerun this audit; one transient `ETXTBSY` flake passed ×3 on rerun)** | 2528/0 at PR-0; 2185/0 at 0.21.9 audit | no | Current-tree green |
| Strict Clippy `-D warnings` (workspace, all-features, all-targets) | **clean (rerun)** | yes | no | Current-tree green |
| fmt | **1 file drift** (`composition/blocking.rs`, untracked pre-audit state; NOT fixed — no-modify rule) | clean historically | no | **drift recorded** |
| Architecture / naming-guard / acceptance | 30 pkgs / 36 frozen / 23/23 (all rerun this audit) | yes | no | Current-tree green |
| cargo-deny/RustSec | **not runnable locally** (`cargo-deny` not installed); CI runs it; no local evidence for the current dependency set incl. `ort` | v0.23.3 CI green on its (pre-voice-workspace) set | no | **not run locally** |
| MSRV / Linux x86_64 / ARM64 / macOS Intel / AS / Windows | none for this tree | five-target green at v0.23.3 commit only (which predates all voice work beyond PR-0's committed slice) | no | **not run** |
| Exact-commit CI / release / package / installer / installed-app acceptance | none | v0.23.3 release green on `94ed16d` | — | **OPEN (PR-5)** |

Note: v0.23.3's release evidence **cannot** be credited to the current tree:
the entire NPU/continuity/acceptance surface postdates that commit and is
uncommitted.

## §F User-evidence timeline (contradictions preserved)

| Date | Evidence | Candidate/build |
|---|---|---|
| ~Mar (2026-03-03 label, unverified) | initial robotic/system voice; STT garbling | system-engine baseline |
| pre-Sep 21 | Kokoro near-silent output → PCM-scaling repair → one clear playback confirmed | early Kokoro |
| Sep 21 | "first spoken 20 s / 40 s / 20 s, Preview 10–15 s" (unacceptable) | CPU acceptance candidate lineage |
| Sep 21 | "ok its way faster then before" | latency-performance candidate |
| Sep 21–22 | repeated `player pipe write failed` → multiturn repair | multiturn-repair candidate |
| Sep 22 00:22 | "still a delay but much better… good enough for now" (continuity/list accepted; ~5.2 s cold gap measured) | `continuity-phoneme-repair` `9d59189a…` |
| Sep 22–23 | NPU Verify failed ("read failed or timed out") → repaired (device-context exhaustion, false-timeout label) | FLM candidates |
| Sep 23 | **"Cooooollll all working with npu and there is not gaps its sounds natural"** | **unknown artifact — acceptance confirmed, association pending** |
| Sep 24 | R6 device matrix PASS: one-press/repeated interruption, explicit Stop and later-F9 recovery; old speech resumed NO; replacement submitted once; Stop capture NO | behavior receipt supplied without a separate executable checksum |
| Sep 24 | **“Looks like smooth”**; pronunciation remains accepted with no new regression reported | voice-quality candidate `f3b736f6…` |

No flattening: the 20–40 s and "still a delay" results are historical
facts about their builds, superseded as *present* status only.

## §G Documentation drift findings

1. `migration-status.md` VRO-17 row — **stale current-status claims,
   superseded but not labeled**: "no NPU route is implemented or
   advertised", "NPU STT … remain open", "the ~1 GB model is absent and
   consent-gated" — all false for the current tree (FLM STT implemented,
   model installed, route registered behind verification), and each sits
   in the same cell as the 2026-09-23 acceptance. The row's closing "Open:"
   list is accurate.
2. PRD §0/§5/§7 headline texts — **historical, correctly scoped** (staged
   plan + decision record; explicitly marked per-stage), EXCEPT:
3. PRD §2.7 "local TTS remains an open candidate, not selected" — **stale
   current-status claim**; superseded by the Kokoro implementation +
   listening acceptance (which that PRD's own later sections record).
   Needs a superseded-label, not deletion.
4. PRD §5 PR-4 "**OPEN: user-operated device acceptance**" — **superseded
   but not labeled** by the later listening/acceptance records this same
   PRD indexes (2026-03-03 continuity + 2026-09-23 NPU).
5. PRD §5 "Current NPU requirement and readiness" paragraph ("speech
   model is not installed… Neither stage is implemented") — **stale
   current-status claim**; contradicted by the R16 table 10 lines below.
6. `voice-cpu-production-acceptance.md` §"Alex's real-device acceptance:
   PENDING" — **historical at its date, correctly scoped** (its own
   verdicts section separates automated from pending).
7. `voice-oracle-kokoro-implementation.md` header "Alex's neural
   listening/F9 acceptance remains OPEN" — **superseded but not labeled**
   (later records closed listening).
8. Evidence-index older entries — read as historical once their dates are
   visible; acceptable, no change needed beyond the audit link.
9. `AGENTS.md` (root) line "production registers Z.ai, LM Studio, and
   native OpenAI" — accurate for *model* providers; no voice-provider
   conflation exists. No drift.
10. The R16a traceability cell in the PRD matrix itself still carries a
    doubled paren from the Verify-repair edit ("`b289a6c1…`)"). Cosmetic.

**Amendment-unit disposition (2026-09-23):** items **1, 3, 4, 5, 7 and
the cosmetic 10** are corrected in this reconciliation unit
(migration-status row rewritten; PRD §2.7 header/resolution, PR-4 phase
line, NPU-readiness paragraph, latency-acceptance header scoped
historically; Kokoro-record header labeled; doubled paren fixed).
Items 2, 6, 8, 9 need no change. The fmt drift (blocking.rs) remains
recorded-only: source files are out of scope for a docs-only unit.

## §H Binary completion decision (AMENDED — counts R1–R20 requirements only)

Counting method (explicit, to avoid the earlier phase/requirement mixing):
R16 contributes **three** sub-requirement rows (R16a STT, R16b TTS,
R16c CPU-usability policy), so the matrix has **22 verdict rows** for the
20 requirement IDs. PR-0…PR-5 phase gates are counted **separately** in
§D and never inside the R-totals.

**A. Is the current (amended) VRO-17 PRD fully complete?**

# **NO.**

**B. R1–R20 verdict ledger (amended contracts):**

| Verdict | Count | Rows |
|---|---|---|
| **PASS** | **17** | R1, R2*(amd)*, R3*(amd)*, R4*(amd)*, R6, R8, R9, R10, R11, R12, R13, R15, R16c, R17, R18, R19, R20 |
| **PASS — scoped limitation** | **4** | R5, R7, R14, R16a |
| **OPTIONAL / capability-gated — conformant** | **1** | R16b (amended: no compatible route established; CPU conformant; nothing binding remains) |

17 + 4 + 1 = **22 verdict rows** = 19 requirement IDs (R1–R15, R17–R20)
+ R16's three sub-rows (R16a/b/c). ✓ The ledger is mechanically derived
from the matrix cells. Historical transitions remain evidence: R20 and R4
closed first; the §2.4 reconciliation correctly reopened R6 after finding the
production binding gap; `voice-r6-binding-repair.md` repaired it; Alex's later
device acceptance closes R6. PR-5 remains a phase gate and is never counted in
the R1–R20 total.

Counting rules (explicit): each matrix row carries exactly one verdict. R4,
R6 and R20 are closed requirement rows. PR-5 is a **phase gate**, excluded
from R-totals and reported in §D.

**Exact remaining OPEN requirement items (post-amendment): none.**

- **R4** — **CLOSED 2026-09-23**: Alex approved Option B; R4 amended to
  an optional STT capability and implemented (capability-aware Settings
  with truthful unavailability for every current final-only backend,
  preference preserved, and the two-fake-provider neutrality guard the
  decision record specified — closing its "PASS — test gap" finding;
  `voice-r4-optional-partials-execution.md`, candidate `673f9fb8…`).
  FLM remains honestly final-only
- **R6** — **CLOSED 2026-09-24**: the §2.4 production binding repair routes
  one Speaking+F9 gesture through playback/synthesis stop, normal runtime
  cancellation and immediate replacement capture; explicit Stop opens no
  capture. Alex then confirmed one-press and repeated interruption, exactly one
  replacement turn, no old-speech resume, explicit Stop, and later-F9 recovery
  on the tested setup. The accepted Ctrl+C cancellation message is not an R6
  failure (`voice-r6-device-interruption-acceptance.md`).
- **R20** — ~~default-build dictation capture bounds~~ **CLOSED
  2026-09-23**: `voice-r20-default-capture-repair.md` (red→green, both
  build flavors on the one managed store, PTY-proven, candidate
  `70cfb122…`).

**Exact remaining phase/release gates:**

- **PR-5** — host/docs/release-feature work is implemented; exact-commit
  canonical/MSRV/five-target/web-driver/release workflows, tag, public asset
  verification and registry update remain

Scoped-limitation notes that no longer gate completion but remain honest:
R7/R14 cloud-path exercise activates only if a cloud adapter is ever
added; R5 has instrumentation (stage footer + core `stage_ms`) and **no
numeric threshold exists** — none is invented; R16a keeps its
correlation-level placement scope and pending artifact identity.

**C. Product choice vs defect (post-amendment):**

| Item | Class |
|---|---|
| R4 partials wiring | **closed at amended optional-capability scope** |
| R6 interruption device acceptance | **closed — user-operated acceptance passed on the tested setup** |
| R20 default-build caps | **closed by the managed-store repair** |
| PR-5 | **release/CI evidence + documentation** |
| Cloud STT/TTS | **future optional features** through `VoiceStt`/`VoiceTts`; no concrete provider ships or is advertised in v1 |
| NPU TTS | **optional capability-gated integration**; no route is claimed |
| fmt drift (1 file) + §G stale texts | **documentation drift** (§G items 1,3,4,5,7 are corrected by the amendment unit; cosmetic cell already fixed) |
| cargo-deny local runnability | **tooling** (CI covers it) |

No missing-composition defects were found in the audited reachable paths.

**D. Shortest path to closure (post-amendment, dependency-ordered):**

1. ~~R20 repair~~ **done 2026-09-23**.
2. ~~R4 scope decision + wiring~~ **done 2026-09-23** (Option B approved;
   amended contract + capability-aware Settings + neutrality guard).
3. ~~Alex R6 acceptance session~~ **done 2026-09-24**: real-device
   interruption/recovery matrix passed at the bounded scope recorded above.
4. **PR-5 unit in progress**: ACP exclusion/parity, user docs and release
   feature repair are done; commit, exact-commit gates, tag, publication,
   asset verification and registry update remain.
5. Optional, only on request: cloud STT/TTS or NPU-TTS integration units
   behind their own gates.

## §I What this audit changed

- Original audit unit: created this report, added the evidence-index
  entry and the foundation ownership line (discoverability only).
- **Amendment unit (same day, Alex-approved):** updated §B rows R2/R3/
  R16b to the amended contracts, rewrote §H with explicit counting
  (22 matrix rows / 20 requirement IDs; phases counted separately),
  refreshed the §G list (items 1, 3, 4, 5, 7 and the cosmetic cell are
  corrected by the amendment unit in the PRD/migration-status), and
  applied the corresponding PRD/migration-status/evidence-index edits.
- **Nothing else across both units**: no production code, tests,
  manifests, dependencies, settings, models, caches, processes, installed
  binaries, or workflows touched; no microphone/speaker/NPU/provider
  workloads run. The fmt drift remains recorded, not fixed (source files
  are out of scope for a docs-only unit).
- **2026-09-24 R6 acceptance closeout:** updated the current R6 verdict,
  corrected the stale R4/R20/R6 ledger accounting to 17 PASS + 4 scoped +
  1 optional/conformant, and linked the device/quality/pronunciation user
  evidence. Documentation only; PR-5 was not started.

## §E′ Verification commands actually run this audit

`git rev-parse/status` (baseline); greps over adapters/registries/ACP;
`cargo xtask architecture` (30 pkgs OK); `cargo xtask naming-guard` (36
frozen OK); `cargo xtask acceptance` (23/23 OK); `cargo build
-p agent-vesper-tui --no-default-features` OK; `cargo build --release -p
vesper-voice --no-default-features` OK; `cargo test --workspace
--all-features` → 2838/0 (187 suites; one `ETXTBSY` flake in
`pr1_adapters` passed 3× consecutively on rerun); `cargo clippy
--workspace --all-features --all-targets -- -D warnings` clean; `cargo fmt
--all --check` → 1-file drift recorded. `cargo deny` not installed locally
— recorded as not-run, not fabricated.
