# Voice Oracle Reconnaissance and Architecture Design Execution

Work unit: analyze the cloned voice-oracle repository as a reference
implementation for a real-time bidirectional voice interface; produce an
evidence-backed architecture report and design an equivalent Vesper-native
subsystem with a migration plan. **No implementation** — the directive
explicitly withheld it until the architecture and migration plan are
complete.

## Objective

Deliver, without a source-code port and without the upstream's brand
names:

1. A mechanism-level analysis of the reference: architecture, STT pipeline,
   TTS pipeline, streaming behavior, session management,
   interruption/barge-in logic, audio lifecycle, concurrency model, error
   handling, configuration, and the agent-runtime integration boundary.
2. A Vesper-native subsystem design preserving provider neutrality, with
   interchangeable local/cloud STT and TTS providers, containing no agent
   reasoning or tool-execution logic, integrated through stable
   interfaces.
3. A staged migration plan with gates.

## Methods and commands

Read-only analysis of the pinned upstream clone at
`/home/Alex/Projects/<upstream>` (name withheld per the alias rule), plus
read-only inspection of Vesper workspace surfaces that the design targets:

```
git -C <upstream> rev-parse HEAD      # 88998de8369e9d36f6d434b5e01feb93fcf1c33f
git -C <upstream> log -1 --format=%ci # 2026-06-13
wc -l <upstream>/server/server.py ...  # component inventory
grep -n <symbol> <upstream>/server/server.py  # line anchors for citations
# Vesper side: crates/vesper-{provider,runtime,agent,harness,config},
# apps/agent-vesper-tui/src/voice.rs + voice_transcribe.py, docs/ tree,
# xtask naming-guard, docs/migration-status.md, Cargo.toml members
```

The full upstream `server.py` (1,241 lines) was read end-to-end, as were
the worker, client, plugin, docs, config, and the HUD's audio/proTOCOL
sections. Vesper integration points were verified against current source
(`ProviderStreamEvent`, `ProviderSession`, `RuntimeSupervisor`,
`SessionTurnResult`, `WebService`, `voice_venv_root`, `VoicePhase`,
`drain_voice`, Cargo members, naming-guard token list).

## Files

Created:
- `docs/architecture/recon_voice_oracle.md` — evidence-backed reference
  analysis (this unit's primary evidence).
- `docs/voice-oracle-extraction-prd.md` — VRO-17 PRD: Vesper-native
  subsystem design and staged migration plan.
- `docs/foundation/voice-oracle-recon-execution.md` — this report.

Modified (indexes/DOX):
- `docs/foundation/evidence-index.md` — two entries appended.
- `docs/AGENTS.md` — ownership bullets for the two new documents.
- `docs/architecture/AGENTS.md` — documented the directive-authorized
  source-reading deviation for this mission.
- `docs/README.md` — spec list link.
- `docs/migration-status.md` — VRO-17 PLANNING row.
- `docs/architecture/recon_voice_oracle.md` Verification section —
  workspace-path existence check list.

No production code, fixture, or config was created or modified.

## Exact evidence

**Upstream inventory** (measured at the pinned commit): `server/server.py`
1,241 lines; `server/hud/index.html` 885; `client/client.py` 378;
`worker/stt_server.py` 76; `worker/worker_stats.py` 70; the agent plugin's
4 files (~140 lines); `docs/ARCHITECTURE.md` 112; config example 77. License
MIT, holder recorded in the pinned commit's `LICENSE`.

**Key mechanisms verified with line anchors** (full table in the recon
report): STT failover `transcribe()` `server.py:319-346`; remote-worker
unavailable-vs-error `server.py:348-363`; local Whisper lock+empty-on-
exception `server.py:330-342`; partial scheduling thresholds
`server.py:1119-1141`; agent SSE event vocabulary `server.py:253-291`;
session persistence/recreate-on-404 `server.py:196-217`, `412-431`;
sentence gate `server.py:476-568`, `595-599`; TTS request/cancel hygiene
`server.py:437-466`; secret redaction set `server.py:93-98` + `_clean_for_tts`
`server.py:604-616`; turn task spawn `server.py:1173-1174`; barge-in
handler `_cancel_active_turn` `server.py:1095-1117`; interrupt-note prefix
`server.py:1071-1078`; ConnState `server.py:1042-1053`; exactly-once warm
`server.py:630-669`; odd-byte PCM carry `hud/index.html:528-531`; barge-in
client behavior `hud/index.html:605`, `client/client.py:230-258`.

**Gap found and recorded:** the config schema documents an acknowledgment
feature (`ack_after_seconds`, `ack_texts`) that no code path reads —
grep over `server/server.py` returns zero uses. Carried into the PRD as a
"do not inherit aspirations without implementation" rule.

**Vesper-side facts the design rests on** (verified in current source):
`ProviderStreamEvent::{ContentDelta, ToolCallStarted, Completed}`
(`crates/vesper-provider/src/stream.rs:41-105`); `ProviderSession::start`
signature with `Arc<dyn CancellationSignal>`
(`crates/vesper-provider/src/ports.rs:217-234`); `SessionTurnResult`
outcome/assistant-content fields (`crates/vesper-runtime/src/session.rs:157-169`);
`WebService` opt-in/byte-identical-when-absent precedent
(`crates/vesper-harness/src/web_service.rs:1-60`); existing dictation
surface and its Python sidecar with binding `vad_filter=True`
(`apps/agent-vesper-tui/src/voice.rs`, `voice_transcribe.py`,
`docs/voice-trailing-silence-vad-prd.md`); naming-guard token mechanism
(`xtask/src/main.rs:1177-1240`); `Cargo.toml` member list and crate
dependency contracts (`crates/AGENTS.md`).

## Deviations

1. **Upstream production source was read**, departing from
   `docs/architecture/AGENTS.md`'s docs/config-only default. The mission
   directive explicitly ordered a full architecture analysis of the
   implementation ("Identify its STT pipeline… concurrency model…"), which
   cannot be done from docs alone. The deviation is recorded in that
   AGENTS.md so the contract stays truthful. MIT license; pinned commit;
   concepts only extracted.
2. **Upstream commit is HEAD of the clone**, dated 2026-06-13, matching
   the clone Alex supplied. No fetch was performed; the analysis is
   truthful for exactly this commit.
3. No `request_human_input` was used: the directive's constraints (no
   port, provider-neutral, no agent logic in voice, no implementation yet,
   voice-oracle alias) resolved all planning choices; remaining open
   questions are recorded in PRD §7 for PR-0 instead of blocking.

## Unresolved items

- Implementation itself — intentionally not started (directive).
- PRD §7 open questions (cancellation-token placement, dictation re-host
  PR, cloud vendor choice) — resolved at PR-0/PR-1/PR-2 respectively.
- The `ack` gap: if Alex wants spoken acknowledgments during long agent
  turns, it must be designed and built, not inherited from upstream docs.
- Native (non-sidecar) audio capture and any wake-word capability remain
  deferred non-goals.

## Readiness effect

The architecture and migration plan for VRO-17 are complete and
evidence-backed; the workspace now has the reference analysis, the PRD,
and the indexed records required to begin PR-0 when authorized. No
behavioral or capability claims about Vesper changed: nothing is
advertised, and both hosts remain untouched.

## Verification

- Docs-only work unit: per house preference, content/link/whitespace
  review was performed; no test suites or release pipelines were run.
- All workspace paths cited in the recon report verified to exist.
- Upstream line anchors re-checked against the pinned working tree during
  the mission.
- DOX pass completed: `docs/AGENTS.md`, `docs/architecture/AGENTS.md`,
  `docs/README.md`, `docs/migration-status.md`, and
  `docs/foundation/evidence-index.md` updated; no stale or contradictory
  text introduced.
- Status: complete for the ordered scope (analysis + design + plan);
  implementation explicitly withheld.
