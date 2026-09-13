# Dollar-token skill routing repair

Date: 2026-09-13. Status: repair implemented and repository verification passed; unreleased.

## Objective and diagnosis

Repair the pasted-prompt failure reported after the PC resumed from sleep:
`skill routing failed: skill '\ge$' was not found`.
The shared router treated every whitespace-separated dollar token as an explicit
skill request. A prompt containing LaTeX `$\ge$` reproduced the identical error
without any suspend/resume event. The original full pasted prompt was not supplied;
sleep itself is not established as a cause.

## Changes and files

- `crates/vesper-memory/src/skill_orchestrator.rs`: recognize dollar shorthand
  only for complete installed catalog names, using original prompt text before
  normalization can turn path separators into hyphens. Skip unrelated dollar
  tokens and continue scanning for a later valid skill mention. Explicit named
  requests and the typed skill field retain missing/ineligible error behavior.
- `crates/vesper-memory/tests/skill_routing.rs`: regression cases for LaTeX,
  currency, shell syntax, unknown dollar names, valid shorthand after math,
  suffix/path rejection and missing explicitly requested skills.
- Owning memory AGENTS records the parsing contract. This report, the evidence
  index and the context-paging PRD record repair scope and evidence.

## Evidence and commands

- Before fix: `cargo test -p vesper-memory --test skill_routing
  dollar_literals_do_not_block_prompt_routing --offline` failed 0/1 with
  the same missing `\ge$` skill. Log: `/tmp/vesper-dollar-before.log`.
- After fix: `cargo test -p vesper-memory --offline` passed; final strengthened
  regressions are included in repository verification below.
- `cargo fmt --all`; `cargo xtask verify` exited 0: 2,444 passed, 0 failed,
  36 ignored across logged test summaries; all 23 exact acceptance cases passed.
  Formatting, clippy and repository verification gates passed. Log:
  `/tmp/vesper-dollar-verify-full.log`. Initial sandboxed
  verification stopped at an ACP loopback fixture with `Operation not permitted`;
  rerun with approved socket permission. No test weakened or skipped.
- TUI `src/main.rs` calls `memory_stores.orchestrate_skills`; ACP `src/lib.rs`
  calls the shared `orchestrate_skills` service, whose `SkillStore::orchestrate`
  owns this parser. Both host paths therefore receive this repair without
  presentation or provider-specific changes. No live provider calls were used.

- `cargo +1.88.0 test -p vesper-memory --test skill_routing --offline`: 14 passed,
  0 failed. `git diff --check` passed; changed documentation links resolve.

## Limits and readiness

Dollar shorthand for unknown names now remains ordinary prompt text. Users who
intend a missing skill receive a clear error through `/skill <name>` or
`use skill <name>`. A literal dollar token identical to an installed skill name
remains shorthand. This is not a general Markdown or shell parser.
No suspend/resume test or original full-prompt replay was performed. No version
bump, release or installed executable replacement is part of this repair.

## DOX closeout

Read root, crates/memory and docs/foundation contracts. Updated the nearest memory
contract; parent and documentation AGENTS remain unchanged because ownership,
dependencies and child boundaries do not change. The owning PRD and evidence index
link this record. No changes to ranker arithmetic or chunk selection rules.
