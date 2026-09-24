# Evidence-led continuity and formatting-unit repair: sentence-level successors and context-aware list-marker handling

> **Scope:** implements the two repairs proposed by the
> [continuity/phoneme reconnaissance](voice-continuity-phoneme-recon.md):
> (1) speech subdivision superseding the 28/48 micro-cut decision, and
> (2) context-aware formatting-only unit handling in the hygiene gate. Bounded
> VRO-17 correction — not full acceptance; Alex's listening verdict remains
> the boundary.

## Baseline verification (§1)

```text
commit:  8f258ba28f4ea2f749526fb32b5b180ecc7dcead (dirty workspace preserved; 97 entries at start)
prior candidate: target/voice-candidates/agent-vesper-tui-multiturn-repair
         sha256 7bd31168b25817b495406581982dad0f83c787cbb2dc4cb7ab690c374a6fce6a
         (resolved in full; identical to target/release at unit start)
recon reproductions re-confirmed RED before the fix:
  - numbered list -> unit "1." -> ZERO phoneme ids (the exact device error)
  - 12-piece 28/48 split of the fixed passage -> 7.2 s stacked fade-silence
owners (re-confirmed, unchanged): hygiene gate (sentence state) -> speech
worker (subdivision + scheduling) -> Kokoro engine (one session) ->
PlaybackOwner (one stream per segment). No duplicates added.
requirement mapping: R5 onset+continuity (both preserved; subdivision
decision replaced as recorded below), R2/R11 truthful errors (formatting-
only omission via markers; meaningful failures still surface), R6/R8
cancellation/settlement untouched, R12 no private text in telemetry, R15
playback receipts unchanged, R16 CPU policy untouched, R17–R20 budgets
unchanged.
```

**Superseding decision (explicit):** the 28/48-character piece policy from
the latency repair is replaced by *clause-sized first piece (for onset) +
sentence-level successors (for continuity)*, with the same bounded
clause/word fallback where the phoneme-context (510-ID) or waveform budgets
require it. The old policy's test was replaced with stronger outcome
coverage (reconstruction identity, boundary classes, onset protection,
terminal uniqueness), not weakened. Historical receipts for the old policy
remain accurate for their scope.

## §2 Continuity repair — what changed

`apps/agent-vesper-tui/src/voice_speech_worker.rs::speech_pieces`:

- **First piece:** unchanged onset rule — a genuine clause boundary
  (`,`/`;`/`:`/`—` + whitespace) within the existing 28-char target,
  12-char minimum, word fallback. NOT an arbitrary four-word cut; where no
  clause exists the honest word boundary remains.
- **Successors:** sentence-level. The next `.!?。`-terminated sentence is
  ONE piece when it fits `SENTENCE_TARGET_CHARS = 510` (aligned to the
  model's 510-phoneme-ID context); over-budget sentences fall back to the
  existing clause/word cuts (bounded, reported honestly).
- One worker/model session/lookahead/stream per segment — all unchanged.
  One terminal outcome per segment preserved (pipeline suite re-verified).

**Measured on the repaired production path** (same passage/voice/profile/
threshold ±400/32768; `repaired_boundary_receipt`):

```text
repaired policy: 5 pieces (was 12)
TOTAL infer=25.0s audio=38.0s agg-RTF=0.66 (was 0.64 — unchanged within noise)
boundaries: 3 natural (sentence) + 1 artificial (the onset piece's cut)
stacked fade-quiet: 2.9s total (was 7.2s) — the artificial mid-sentence
  insertions dropped from 11 to 1 (the single onset cut)
```

**Paced production run** (`continuity_pause_receipt`, release candidate):

```text
[long-answer] first PCM t+3.16s; total 47.0s; audio 38.05s; 5 pieces
consumer read intervals: p50=p90=p99=0.128s (exact pace)
read gaps > 0.4s: 1 — a 5.16s gap at the FIRST sentence-level successor
```

**Residual cold-path risk, quantified and NOT slipped in:** the first big
successor (162 chars) infers ~7.7s while the onset piece plays ~2.7s of
audio plus the 2.0s pipe — one starvation window of ~5.2s. The bounded
remedy would be audio-duration prebuffer (~2 successors ≈ 384 kB, far
under the 16 MiB/piece bound) applied after the first piece; **that is a
separate production buffering change requiring review**, per the directive.
Everything after the first successor runs gap-free (p99 = exact pace).
RTF definition held constant: synthesis time ÷ generated audio duration,
quiet included on both sides of the comparison; amplitude-threshold quiet
remains an approximation, not phonetic silence.

## §3 Formatting repair — what changed

`crates/vesper-voice/src/hygiene.rs`:

1. **`leading_list_marker`**: digits+`.` followed by whitespace (or
   end-of-pending, where an item may still stream) at a sentence start —
   start-of-pending, after `\n`, or after sentence-ending whitespace
   (streamed prose often has no newlines; mid-sentence numbers like
   "with 3 fixes" never match because the preceding text does not end a
   sentence). ≤3 digits so `3.14`/`v2.4.1`/`2026` never match.
2. **`find_sentence_end`**: a matched marker's own period is skipped as a
   sentence boundary — the marker stays attached to its item
   (`"1. Do the first thing"` is one unit).
3. **`drain_sentences`**: a drained unit that is ONLY a presentation
   prefix (bare `N.` marker, `#`-only heading delimiter, single `-`/`*`/`+`
   bullet, `|---|`-shape table rule) defers (stays pending) instead of
   emitting; more text joins it.
4. **`finalize`**: an unresolved bare prefix at end of turn records
   `HygieneMarker::SkippedSpan { class: "formatting-only" }` — no empty
   `SpeakUnit`, no phonemizer/model call, no fabricated audio, no
   "formatting omitted" announcement, no stranding of later units.

**Mandatory distinctions, pinned by tests** (`hygiene_formatting_units`,
13 tests): `## Results.` keeps `Results`; item text survives verbatim with
its marker; `The answer is 1.`, `42`, `3.14`, `v2.4.1`, `2026`, and
`See item 2` never lose content; table header/data cells survive while
separator rows never emit; symbols-as-content (`^ ~ |` in prose) still emit
(a visible engine failure if unspeakable — never pre-dropped); formatting-
only whole turns emit nothing; a normal passage after an omitted fragment
speaks. Every small reproducer is asserted at EVERY valid character
boundary (except the documented pre-existing decimal-split limitation,
below).

**Red→green receipts:** pre-fix, `numbered_list_marker_joins_its_item`
and `dense_numbered_list_markers_join` failed with the gate emitting
standalone `"1."` units (the device error); post-fix, 13/13 green.
Post-fix PTY (full production path, word-sized streamed deltas):
`speech failed` absent, real Kokoro PCM delivered, all list text spoken.

**Open finding (pre-existing, not introduced):** a chunk boundary that
splits a DECIMAL at `"N.|rest"` (e.g. `"Version 2." + "4.1 ships"`) lets
the end-of-pending period terminate a sentence mid-decimal. This predates
this unit (the same shape caused the `"1."` bug this repair fixes for
markers) and needs general end-of-pending terminator holdback — recorded
for a future bounded fix, not silently waived.

## §4 Production-path proofs

```text
r3_loop_pty.py (isolated loopback provider, recorder/STT/player doubles):
  list mode (NEW — realistic streamed numbered answer, word-sized deltas):
    PASS — no phoneme failure; PCM bytes=[62400,160000]; peaks=[21808,29941]
  off (direct): PASS (unchanged shape)          balanced (VRO): PASS
  preview (Settings → pack screen): PASS, click-to-first-PCM=2598 ms
voice_multiturn_playback: 11/11 (repeated-turn + death-recovery intact)
voice_speech_pipeline: 1/1 (overlap/stop/settle; waits extended to cover
  one sentence-level successor inference in the espeak fixture's own
  modeled 6s latency — invariants unchanged, piece-count assertion
  updated to the superseding policy with onset protection kept)
voice_interruption_lifecycle: 5/5 (five Stop/recovery cycles incl. during
  successor synthesis; session exit; preview lifetime)
cross-chunk secret/reasoning/code/PEM + finalization fixtures: green
  (85/85 vesper-voice lib; 13/13 hygiene_formatting_units)
```

## Gates and artifact (§6)

```text
candidate: target/voice-candidates/agent-vesper-tui-continuity-phoneme-repair
build:     cargo build -p agent-vesper-tui --features voice-kokoro --bin agent-vesper-tui --release
sha256:    9d59189a00999fffc5bde5994c49ab7061ecda380c7d7f1e4698522052b54271
prior candidates preserved: 7bd31168… (multiturn), d633b4c3… (cpu-acceptance)
```

| Gate | Result |
|---|---|
| vesper-voice lib / suites | **passed** (85 / 13+13+29) |
| vesper-voice-kokoro lib | **passed** (9) |
| TUI voice lib / default lib | **passed** (279 / 242) |
| voice_multiturn_playback / pipeline / lifecycle / parity | **passed** (11 / 1 / 5 / 7) |
| pr4_f9_gate / pr4_wiring / r3_voice_pack / execution_policy | **passed** (10/10/10/10) |
| hygiene_formatting_units (new) | **passed** (13) |
| PTY all four modes on the repair candidate | **passed** |
| clippy `-D warnings` (vesper-voice all-features, TUI voice-kokoro, kokoro ort) | **passed** |
| architecture / naming-guard / acceptance | **passed** (30 packages / 36 frozen / 23) |
| rustfmt (all changed files) | **passed** |
| workspace fmt | pre-existing `ui.rs` lines only (documented) |
| MSRV / cross-target / CI / cargo-deny | **not run** (unchanged status) |

Storage: temp fixtures removed (0 residue); pack untouched (118 009 413 B);
disk 262.4 GiB free; no downloads/devices/settings changes; CPU execution
policy untouched (registry still empty — CPU route for both policies).

## Verdicts (separate)

1. **Implemented corrections:** both delivered — sentence-level successor
   subdivision (onset preserved) and context-aware formatting-unit
   handling (meaning preserved; no silent loss).
2. **Automated/real-model software verification:** passed for the tested
   scope (all suites + four PTY modes + real Kokoro inference receipts).
   Residual quantified: one ~5.2s cold-start gap at the first big
   successor on a paced sink; the bounded prebuffer remedy is proposed
   separately for review, not applied.
3. **Alex's listening acceptance:** PASSED for this bounded repair on
   2026-03-03. Alex reported that a delay remains, but it is “much better then
   before” and “good enough for now.” Residual delay remains open.

## Listening comparison (one sitting, on the new candidate)

```sh
cd /home/Alex/Projects/agent-vesper
./target/voice-candidates/agent-vesper-tui-continuity-phoneme-repair
```

1. An ordinary short reply (onset should feel as prompt as before).
2. A longer reply with numbered steps/headings — continuity between
   sentences, no failure on the list, meaning intact.
3. Interrupt once mid-speech, then ask something else — old speech stops,
   the next reply speaks.

STT garbling, NPU STT/TTS, cloud commitments, and PR-5 remain separate
open work; this repair does not close them.

## Listening-acceptance closeout

The full device receipt, root causes, regression-first evidence, measured
tradeoff, commands, deviations, and unresolved items are in
[the listening acceptance report](voice-continuity-listening-acceptance.md).
The accepted implementation replaces routine 28/48-character successor cuts
with a bounded early piece plus sentence-level/remainder successors, and keeps
recognized streamed list prefixes with their item so `1.` cannot become an
empty standalone Kokoro request. Meaningful numbers and unexpected
zero-phoneme failures remain truthful. The measured change was 12 pieces to 5,
about 7.2 s to 2.9 s stacked fade quiet, 11 to 1 artificial mid-sentence
insertions, and unchanged aggregate RTF near 0.66. Residual delay, including
the measured ~5.2 s first-large-successor cold-start gap, remains open.
