# Explicit evaluation launchers

## Purpose

Own manually invoked, non-shipped evaluation entry points that compose real adapters.

## Ownership

- `skill_routing_eval.rs` prepares the frozen routing corpus without provider calls
  by default. `--execute-live <new-output.jsonl>` explicitly runs the bounded Z.ai
  GLM-5.3 coding-plan selector using existing adapter credential resolution.

## Local Contracts

- Live execution requires separate user authorization. Foundation verification may
  compile this example but must never pass `--execute-live`.
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
