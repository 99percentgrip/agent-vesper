# Vesper Bridge (docs/Vesper bridge/)

## Purpose

Own the VB-PRD-001 revision 1.0 product requirements, the Phase 0
reconnaissance evidence package, and future Bridge phase evidence.
The PRD is the frozen requirements authority; nothing in this directory
may restate or weaken it. The `.docx` alongside the `.md` is the
original authoring artifact.

## Ownership

- `Vesper_Bridge_PRD.md` — frozen original scope (do not edit; the
  acceptance enrollment digest is computed over it).
- `PRD-ENROLLMENT-NOTE.md` — mechanical record of the enrollment
  paragraph-ceiling blocker (283 vs 256); not a scope substitute.
- `recon/` — Phase 0 evidence: pinned upstream sources with SHA-256
  digests (`SOURCE-DIGESTS.txt`), the phase-0 report (integration map,
  adapter decision record, compatibility manifest, verdict), the threat
  model and AT test plan, and the phased change plan with ADR drafts.
- Phase 2 increment evidence: `phase2-report.md` (harness composition),
  `phase2b-report.md` (host wiring; its harness receipt was later
  corrected by increment 4), `phase2c`–`phase2m` increment reports
  (contract invariants + provenance, MSRV/deadlock repair, ACP AT-01
  OS observation, TUI AT-01 lane + shared `/bridge` command repair,
  core-side bounded state with monotonic request ids, emergency input
  release survival after ordinary revocation, production stop/resume path
  in both hosts, input-release settlement reaching `Released`,
  quarantine lifecycle honest + resolvable via evidence, §7 outcome
  states `Cancelled`/`Partial` reachable, `Paused`/`Closed` producers +
  disconnect surfacing unresolved inputs/jobs), and
  `phase2n-final-audit.md` (hand re-derivation of every claim; corrected
  increment 13's test-edited-to-green defect in place).
- The architecture decisions from `phased-plan.md` are the accepted
  **ADR 0030** (`docs/adr/0030-vesper-bridge-application-control.md`);
  the phased-plan file remains historical drafting evidence.
  AT-01 now holds compiled-out, advertisement and OS-observation
  evidence in both hosts.
- `full-mission-audit.md` (report-only audit before Phase 3) and
  `phase2q-audit-fixes-report.md` (increment 16: every audit finding
  fixed red-first — lease-leak-on-denial, revalidate-before-dispatch,
  quarantine-erasing close, owner-scoped emergency release, per-mutation
  observation freshness, authority epochs, `/bridge disconnect`
  execution, argument/pixel bounds, honest denial detail). The audit's
  fix-status table inside `full-mission-audit.md` is the per-finding
  disposition record; adapter-phase debts (M3, M5, C3's timeout
  producer) are named there with their triggers.
- `phase2r-resolve-live-probes.md` (increment 17: Resolve free 21.1
  live evidence — external scripting Studio-gated (port 1144 never
  listens), internal Console scripting works, Console Lua sandboxed
  (no `io`), Scripts-menu route absent in free, **marker
  request/response channel proven live**, `resolve --version` blocks
  forever — never call unbounded). Probe scripts live in `recon/probes/`.
- `phase2s-resolve-vertical-slice.md` (increment 18: full slice PASS —
  import → timeline → render → frame-exact verified output; free-edition
  defects measured: no H.264 encoders, ImportMedia string-form only,
  CustomName breaks AddRenderJob).
- `phase2t-paste-free-worker.md` (increment 19: **one paste per launch,
  then autonomous file-driven control** via LuaJIT-FFI channel; second
  render executed with zero human involvement and frame-verified).
- `phase2u-elisa-mpris.md` (increment 20: second application — Elisa via
  MPRIS, **40s connect, zero per-app code**, playback verified at three
  layers; the route-ladder economics measured).
- `phase2v-native-adapter-integration.md` (increment 21: `AdapterPort`
  seam, Resolve file-IPC + MPRIS adapters dispatched through the real
  `bridge_execute` authorization path, **live-tested against both running
  applications**; adapters live in `crates/vesper-harness/src/bridge_adapters.rs`).
- `phase2w-dispatch-hardening.md` (increment 22: §7 hardening — timeout
  honesty (`unknown_outcome`, never `failed`) and semantic duplicate
  suppression keyed on operation identity at authorize step 5a).
- `phase2x-latency-investigation.md` (increment 23: latency complaint
  root-caused to test-suite cost — product path 22 ms; enrollment-ceiling
  test override 10s→1s; live latency-budget test added).
- `phase2y-three-track-choreography.md` (increment 24: 30/15/30s
  three-phase choreography PASS with per-transition identity checks;
  Elisa's rapid Next/Previous inversion crash measured and its mitigation
  — spaced calls, forward-only navigation — recorded for adapters).
- `phase2z-adapter-behavior-guards.md` (increment 25: MPRIS adapter
  guards — previous-while-paused refusal, inversion cooldown, trackid-hop
  verification — all traced to measured facts; `MprisBus` test seam).
- `phase2aa-audio-event-stop.md` (increment 26: kick onset located by
  low-band waveform analysis (59.28s), exact-window playback + stop,
  app closed; **MPRIS Seek is relative** — adapter seek must be
  state-aware).
- `phase2ab-first-kick-correction.md` (increment 27: correction — true
  first kick **51.54s** confirmed at 2× threshold, tempo-locked;
  59.28s "drop" was a narrative error; detector lesson recorded).
- `phase2ad-wave-detector.md` (increment 28: `tools/bridge/
  kick_detector.py` — calibrated, blind-validated kick detector;
  sustained-run discriminator fills≤3 vs kicks≥4).
- `phase2ae-fx-fill-choreography.md` (increment 29: FX fill 27.88s,
  half-time kick counting through interleaved patterns, exact 4th-kick
  stop at 59.11s).
- `phase2af-fx-fill-repeat.md` (increment 30: full task with app close;
  both waveform facts independently re-derived; +98 ms stop precision).
- `phase2ag-video-edit-delivery.md` (increment 31: real video edit
  delivered — banner blur verified by edge energy; LinkedIn transcode;
  six measured Resolve-Fusion free-edition limitations recorded).
- `phase2ah-video-edit-v2.md` (increment 32: v1 rejected by Alex —
  region missed lower rows, coverage gaps; v2 blurs the full panel
  square across the entire Alex-confirmed visible window; delivered
  file re-verified).
- `phase2ah-video-edit-v3.md` (increment 33: v2 still left top and
  bottom text rows readable — row-profile scan measured the panel at
  y 0.04–0.93; v3 blurs full window height 174–187s; worker-loop
  Console-blocking behavior documented).
- `phase2ah-video-edit-v4.md` (increment 34: v3 "too big" — box
  re-sized to the text block only; inside 0.11 blur, outside content
  sharp 1.4–5.7).

## Local Contracts

- Upstream citations must point at the pinned copy in `recon/` with its
  recorded digest; rolling docs are re-pinned before they load any
  implementation decision.
- Statuses use PASS / FAIL / BLOCKED / NOT TESTED; a blocked lane is never
  reported as passed.
- No capability claim beyond what a cited test observed, on the recorded
  OS/app/driver combination.

## Work Guidance

- New phase evidence lands as `recon/phase<N>-report.md` following the
  house report convention (objective, methods/commands, files, exact
  evidence, deviations, unresolved items, readiness effect).
- Keep executable provenance (release tags, digests, checksums) for every
  sidecar the design depends on.

## Verification

- Phase reports are reviewed against the PRD's acceptance matrix rows they
  claim; each claim cites the environment it was observed in.
- Digests in `SOURCE-DIGESTS.txt` are re-verified (`sha256sum`) whenever a
  pinned file is touched.

## Child DOX Index

- none yet
