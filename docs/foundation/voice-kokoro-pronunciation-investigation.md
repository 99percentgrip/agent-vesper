# VRO-17 — Kokoro Pronunciation Investigation and Bounded Repair

> **Subsequent user acceptance (2026-09-24):** the pronunciation repair is
> accepted on the tested path. Alex reported no new pronunciation regression
> during the latest short-reply smoothness retest. This does not claim universal
> pronunciation correctness or remove the residual espeak G2P limitation below.

Investigation + repair unit: 2026-09-23 (late). Baseline `main` @
`8f258ba` + the dirty VRO-17 voice tree. At this unit's date R6 remained
**OPEN** and untouched; it later passed device acceptance. No STT/NPU, R20,
R4, provider, or playback changes were part of this pronunciation repair.

## User-observed defects (acoustic TTS evidence, preserved verbatim)

- am_michael: *reply* sounded like **ripple**
- af_heart: *ready* sounded like **read**
- af_heart: *now* sounded like **no**

## Controlled reproduction (direct TTS, no reasoning provider)

Fixtures run through the production phonemizer (the crate's real
`normalize_text` → `split_sections` → espeak-ng → `postprocess_phonemes`
→ vocab → ids) via a temporary probe example (deleted after; no
production logging of user text added):

**Pre-repair trace (the defect, reproduced deterministically):**

| Input (production speech piece) | Phonemes produced | Correct |
|---|---|---|
| `Replying fast now.` | `ɹᵻplˈaɪɪŋ fˈæst nˈoʊ.` | `… nˈaʊ` |
| `I'm here ready,` | `aɪm hˈɪɹ ɹˈiːd,` | `… ɹˈɛdi` |
| `The test` | `ðə tˈɛs` | `ðə tˈɛst` |
| bare `reply` | `ɹˈɛpəl` | `ɹᵻplˈaɪ` |
| bare `ready` | `ɹˈiːd` | `ɹˈɛdi` |
| bare `now` | `nˈoʊ` | `nˈaʊ` |

Mid-sentence occurrences of the same words were **correct**
(`ɹᵻplˈaɪ`, `ɹˈɛdi`, `nˈaʊ`): every corruption lands on the **final word
of the espeak input**.

## Root cause (proven, not assumed)

`run_espeak` sent the joined text sections **without a terminating
newline**. espeak-ng `--stdin` treats missing-final-newline input as a
truncated final line and degrades the last word's pronunciation —
diphthong flattening (`aʊ→oʊ`), final-syllable elision (`ɛdi→iːd`),
final-consonant drops (`tɛst→tɛs`). Isolation proof (shell, byte-exact):

```
printf 'Replying fast now'  | espeak-ng --ipa -q -v en-us --stdin   → nˈoʊ
printf 'Replying fast now\n'| espeak-ng --ipa -q -v en-us --stdin   → nˈaʊ
printf "I'm here ready"     | …                                      → ɹˈiːd
printf "I'm here ready\n"   | …                                      → ɹˈɛdi
```

The section join already emitted `\n` **between** sections — only the
last line lacked it, which is exactly why every defect was
final-word-positioned and why Alex heard them at speech-piece ends
(*now* ended fixture 1's first sentence; *ready* ended the clause;
*reply*'s degradation at a piece tail produces the "ripple"-class
mangling). Segmentation was **not** the owner: piece boundaries keep
words intact (`"Replying fast now. I'm here ready, "` → pieces end at
punctuation, words whole).

## Boundary-by-boundary verdicts

- **Text/hygiene:** PASS — `normalize_text` output equals input for all
  fixtures (spelling unchanged; no abbreviation/number rule fires).
- **Segmentation:** PASS — words intact mid-piece; first-piece clause
  cut lands after `test,`/`ready,`; no lexical split.
- **Phonemizer invocation:** **FAIL pre-repair** (the newline defect);
  PASS post-repair (`ɹᵻplˈaɪ nˈaʊ`, `ɹˈɛdi`, `tˈɛst`, `tˈuːl`).
- **IPA post-processing:** PASS — `postprocess_phonemes` rules
  (r→ɹ, plural-z, ninety, hundred-space) verified against targets; none
  touch these words' diphthongs.
- **Token mapping:** PASS — `ᵻ`(177)/`a`/`ʊ`/`ɛ`/`d`/`i` all in the
  pinned vocab; `stripped=0` for every fixture; ids match phonemes 1:1
  (wrapper `[0,…,0]` contract pinned).
- **Native Kokoro output / voice:** PASS — with corrected phonemes both
  voices receive the intended sequence (the model itself was never the
  defect); voice-specific differences remain model quality, out of scope.
- **Resampling/playback:** PASS — corruption was present **before**
  resampling (in the phoneme string), so the audio path is exonerated;
  resampler unit tests unchanged and green.

## Classification per user-observed word

| Word | Classification | Evidence |
|---|---|---|
| reply → "ripple" | **PHONEMIZER_ERROR** (stdin truncation at piece tail) | bare/piece-final `ɹˈɛpəl`-class mangling pre-fix; `ɹᵻplˈaɪ` post-fix |
| ready → "read" | **PHONEMIZER_ERROR** (final-syllable elision) | `ɹˈiːd` → `ɹˈɛdi` with newline |
| now → "no" | **PHONEMIZER_ERROR** (diphthong flattening) | `nˈoʊ` → `nˈaʊ` with newline |

All three: **Vesper-owned** (the invocation bug), shared across voices,
position-dependent (final word of the espeak input), independent of
first-vs-successor piece synthesis (any piece's last word was exposed).

## Repair (smallest general fix, red→green)

One line in `phonemize_blocking`: the joined sections now get a
**terminating newline** (`format!("{}\n", lines.join("\n"))`) with the
measured proof in the comment. No word lists, no dictionary, no prompt
change, no second phonemizer, no engine/config change.

**Red-first:** four new tests in
`phonemize.rs::pronunciation_repair_tests` pin `nˈaʊ`, `ɹˈɛdi`, `tˈuːl`,
`tˈɛst` and mid-sentence correctness. Reverting the fix → **all four
FAIL**; restored → green. Honest correction recorded: the first test
draft's skip-guard used a relative `espeak-ng` path, which never exists
as a file — the guard silently skipped and the initial "red" run was
vacuously green. Fixed to PATH resolution (the production rule) before
taking the real red receipt.

## Verification

`vesper-voice-kokoro` 42/42 (release, ort) · existing phoneme examples
re-run green (full_path, shape_matrix, markdown_units, numeric_probe,
strand_check — `100.` → `wˈʌn hˈʌndɹɪd` etc. all correct post-fix) ·
`vesper-voice` suites green · TUI feature suites 16 green · clippy
`-D warnings` clean (all targets) · fmt · architecture 30 pkgs ·
naming-guard clean · **acceptance 23/23** · PTY on the candidate:
`r3_loop` PASS (F9 full path, real Kokoro), `voice_pty` PASS (F5),
`flm_f9_loop` PASS (NPU route; no flm leak; registry clean).

## Diagnostic audio (bounded, retained for Alex)

`/tmp/vesper-pron-ab/` — 10 WAVs (16 kHz s16), the six word-context
cells × both voices + both full fixtures: peak/aggregate **1,106,840
bytes (1.1 MiB)** vs caps 8 MiB/file, 32 MiB aggregate. Owned probe
directory; intentionally retained as the listening A/B artifact
(pre-fix phonemes are the recorded A-side evidence above; audio is
post-fix). Cleanup after Alex's verdict, or on request.

## Candidate

`target/voice-candidates/agent-vesper-tui-pronunciation-repair` —
SHA-256 `3b1bb9d34396b9b35371a88563a982c8b78ac0acb9118585157d8846d83b7e52`
(byte-identical to `target/release`), release, features
`voice-flm,voice-kokoro`, source `8f258ba` + dirty tree. All prior
candidates preserved; nothing installed.

## Residual limitation (unchanged, honest)

espeak-ng `--ipa` is not a full G2P: unusual words, names, and
uncovered abbreviations may still be mispronounced — a recorded
limitation of this route, never claimed as parity. Any
pronunciation-override feature needs separate approval.

## Alex's listening matrix (when convenient)

Run the candidate; ask for **correct / still wrong / different but
unclear** on: Michael reply · Michael ready · Michael now · Heart reply
· Heart ready · Heart now, then both full fixtures. His listening is the
acoustic authority; unit tests prove phonemes only. This matrix was the
requested form at the repair's date; the later bounded user acceptance is
recorded at the top of this report and in the R6 device-acceptance closeout.
