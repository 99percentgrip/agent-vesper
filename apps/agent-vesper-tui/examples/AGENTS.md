# Explicit evaluation launchers

## Purpose

Own manually invoked, non-shipped evaluation entry points that compose real adapters.

## Ownership

- `first_speech_receipt.rs` compares a fixed long phrase's whole-unit preparation
  with the production pipeline. `--measure` uses a no-device sink; `--play-one`
  requires explicit listening consent. Worker initialization retains its existing
  pack lease behavior; neither mode is an automatic foundation test.
- `voice_live_timing.py --execute-live` requires separate live-provider consent.
  It runs one fresh native turn with saved model/reasoning choices, fixture
  capture/STT/player and real Kokoro; mutable session/UI/memory roots are isolated.
  Credential resolution stays native; values are never logged/copied. This helper
  currently supports the active OpenAI adapter only, not a production restriction.
  It has a 90-second turn deadline and no automatic retry; it does not independently
  count HTTP requests or model tool calls and is never routine CI.

- `preview_first_pcm_receipt.rs` is a device-free production-worker timing check
  for cold-screen preparation and two repeat Preview attempts; it opens no speaker.
- `short_reply_boundary_receipt.rs` runs one fixed short reply through the real
  installed Kokoro adapter and production worker into a file sink. It measures
  only the artificial piece join, opens no device or network, and is not an
  acoustic acceptance test.
- `preview_latency_receipt.rs` and `speech_pipeline_receipt.rs` are manual
  real-Kokoro output checks. Sound requires explicit user authorization plus
  `--play-once` or `--play-two`, respectively. The latter offers `--serial` as a
  two-sentence scheduling control. No microphone or agent/provider turn is used;
  software timing/player drain never substitutes for human listening evidence.
  `r3_worker_device_check.rs` is a legacy real-output launcher; invoking it plays
  sound and requires the same separate permission. None is routine CI execution.

- `skill_routing_eval.rs` prepares the frozen routing corpus without provider calls
  by default. `--execute-live <new-output.jsonl>` explicitly runs the bounded Z.ai
  GLM-5.3 coding-plan selector using existing adapter credential resolution.

## Local Contracts

- Live execution requires separate user authorization. Foundation verification may
  compile this example but must never pass `--execute-live`.
- Receipt launchers that build executable POSIX shell/player fixtures run only on
  Unix and compile a truthful no-op notice on Windows; all-target CI must never
  compile Unix APIs unconditionally.
- Preserve the skill library, use no tools, perform no installation or settings writes.
- One pass, at most 205 selector calls, 20 seconds each; no automatic retries by this
  launcher. Adapter transport retries remain inside each deadline.
- Results contain IDs, usage and bounded diagnostics, not prompts or skill bodies.
- Corpus resource constraints unsupported by the native fixture remain explicitly
  unsupported; inspected cases are regression evidence, not a fresh holdout.

## Work Guidance

- Do not infer live quality from dry-run or scripted-provider checks.

## Verification

- `cargo check -p agent-vesper-tui --example skill_routing_eval`.
- `cargo run -p agent-vesper-tui --example skill_routing_eval` performs only dry-run preparation.

## Child DOX Index

No children.
