# Skill routing quality implementation

Date: 2026-09-13. Status: **preview implemented; quality promotion HOLD; combined acceptance incomplete**.
PRD: [Skill routing quality](../skill-routing-quality-prd.md).
Checkout: `/tmp/vesper-routing-quality`, branch `feat/skill-routing-quality`.

## Objective and coexistence

Implement natural-task routing while preserving the complete library and GLM's
parallel score-floor work. [Recon](skill-routing-coexistence-recon.md) records
its PR-1 baseline `5cf5835`. GLM PR-2/PR-3 are not assumed complete. The original
checkout remains untouched; no release, push or installation is part of this work.

## Implementation and files

- Memory `routing_quality.rs`, `skills.rs`, `skill_orchestrator.rs`: bounded
  descriptors, twelve-candidate BM25 retrieval, typed action/artifact/effect/resource
  checks, conservative activation, ambiguous/missing-precondition outcomes and
  bounded notices. Standard remains the default. Both paths retain the existing
  three-skill, conflict, inline/fork and shared chunk-loading protections.
- Descriptor sidecars are optional and source-local: local skills shadow global
  skills and their sidecars. Missing descriptors use legacy metadata; malformed,
  conflicting or stale descriptors exclude enhanced automatic activation.
  Source freshness includes bounded prefix, byte length and modification time;
  this is not a cryptographic full-body attestation. Runtime never reads isolated
  bodies for indexing. Index state is memory-only, keyed by live eligible entries;
  live identity is checked before and after selected-body loading.
- Domain `slash_commands.rs` and harness `skill_routing_settings.rs`: shared
  `/skills settings` parser, explicit atomic project saves, disabled IDs,
  no-write default/status and a bounded refinement helper. No provider/network
  dependency was added. SHA-256 is a dev dependency for evaluation receipts only.
- TUI `commands.rs`, `main.rs`, `settings_host.rs`: Skills panel in the existing
  grouped Save/Discard/Keep editing transaction, next-turn/restart preferences,
  shared routing and notices preserved through all three startup paths.
- ACP `lib.rs`: equivalent explicit text saves, same shared selector and
  preferences, mode/permission restriction hints and reasoning-channel notices.
- Memory parsing ignores quoted/code/fenced/blockquoted instructions and negative
  named requests. It preserves the original submitted text and complete-name
  dollar shorthand. Explicit missing/ineligible names still fail visibly.

## Using the isolated preview

Build and launch `target/debug/agent-vesper-tui` from this checkout to avoid replacing
the installed executable. In Settings → Skills choose Enhanced (preview), then
leave Settings and choose Save changes. Returning to Standard reverses the mode;
per-project disabled choices persist independently and never edit skill files.
The ACP/text equivalents are:

```text
/skills settings status
/skills settings save mode enhanced
/skills settings save mode standard
/skills settings save disable <skill-name>
/skills settings save enable <skill-name>
```

These controls do not make the failed quality gates pass. The installed application
will not expose this implementation until a separately authorized integration/build.

## Methods and evidence

All fixtures use temporary roots and no live provider. Build profiles disable
debug information and incremental artifacts after the isolated `/tmp` build hit
its quota; only this checkout's generated target directory was cleaned.

- Literal regression failed before repair on `Explain \`use skill missing\`.`;
  after repair the literal and existing routing suites passed.
- Initial store tests exposed a size-dependent activation threshold; it was
  replaced before the expanded quality matrix with overlap support and a relative
  shortlist margin. No probability interpretation is attached to scores.
- The real TUI PTY passed draft, discard, keep-editing, save, restart, and byte-exact
  source preservation. The first fixture tried a label decoration omitted by the
  renderer; correcting that fixture label made it exercise the actual row.
- The real ACP command test passed after rerunning outside the socket-restricted
  sandbox. Status created no `.agent-vesper`; explicit saves persisted choices;
  no request reached the loopback provider listener.
- Changed-crate and both-host Rust 1.88 all-target/all-feature checks passed
  (`/tmp/routing-msrv-final.log`). This is compilation evidence, not a claim of
  full-workspace MSRV test execution.
- Shared composition tests passed: eight context-paging/routing tests, including
  actual AgentLoop and VRO execution plus supported ReAct composition, and four
  settings/refinement tests (`/tmp/routing-composition.log`). These are contained
  composition fixtures, not live-provider or full native VRO terminal tests.
- Final native Skills and existing Settings PTY suites both passed
  (`/tmp/routing-tui-final-pty.log`, `/tmp/routing-existing-settings-pty.log`).
- 500-entry cost receipt on Linux x86_64, AMD Ryzen AI 9 465 (20 logical CPUs):
  pure-index cold 7.470 ms, warm p95 1.838 ms; complete store cold 58.851 ms,
  warm p95 36.632 ms, maximum 37.250 ms. Ten warm-ups and 100 timed queries.
  `/usr/bin/time -v` measured maximum process RSS 6,692 KiB, including the fixture
  and index. This passes the local 50 ms warm / 2 s cold / 32 MiB limits;
  it is not an all-platform performance guarantee.
- Initial full checks passed Rust suites but failed the documentation naming guard.
  A deferred-tool vendor citation was removed from the new PRD/research addendum;
  the other primary research citations remain. The naming baseline was not changed.
  Final `cargo xtask verify` exited 0: formatting, workspace Clippy with warnings
  denied, workspace tests/doc tests, fixture/contracts/architecture/naming,
  provider/runtime/ACP/session gates and all 23 acceptance cases passed.
  Receipt: `/tmp/routing-verify-closeout.log`.
  The workspace all-feature phase passed 2,279 tests, with 0 failures and 35
  ignored cases. Ignored live-provider/container/browser/soak/performance cases
  were not executed; the GLM PR-2 noise-floor anchor remains explicitly ignored.

Reproduction commands from this checkout, with
`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`:

```sh
cargo xtask verify
cargo +1.88.0 check -p vesper-memory -p vesper-harness -p vesper-domain -p agent-vesper-tui -p agent-vesper-acp --all-targets --all-features --locked --offline
VESPER_ROUTING_EVAL_OUTPUT=/tmp/routing-quality-results.json cargo test -p vesper-memory --test routing_quality_eval -- --nocapture
cargo test -p vesper-memory --test routing_quality_perf -- --nocapture --test-threads=1
python3 apps/agent-vesper-tui/tests/skill_routing_pty.py target/debug/agent-vesper-tui
python3 apps/agent-vesper-tui/tests/settings_pty.py target/debug/agent-vesper-tui
python3 docs/foundation/skill-routing-quality-summary.py docs/foundation/skill-routing-quality-results.json
```

## Quality evidence and limits

The frozen 240 labels contain 80 real positive tasks in 13 families, 40 no-skill,
40 sibling, 40 syntax and 40 resource cases. Sibling/resource procedures are
explicitly synthetic; they do not establish downstream real-world task success.
Two cohorts keep all helpful targets in 25-entry snapshots: real skills for 160
cases, synthetic contracts for 80. Larger snapshots add real and synthetic
near-duplicate/irrelevant distractors. Three insertion orders must agree.

The first Standard/Enhanced matrix completed all sizes. At 500 entries, held-out
positive Recall@3 was 15/40 Standard and 20/40 Enhanced; no-skill false activation
was 3/20 and 1/20; harmful sibling exposure was 4/20 and 0/20. Enhanced still
selected an alternative operation for missing-template tasks. That finding led
to explicit action-contract checks and a regression; original labels were retained.
These initial results **fail promotion**. They are preserved in
`/tmp/routing-quality-matrix.log`, not converted into a success claim.

Final [prediction receipts](skill-routing-quality-results.json) include all sizes,
three insertion orders and four conditions; the
[summary helper](skill-routing-quality-summary.py) reproduces metrics and failures.
No labels were changed. Conditions are Standard, descriptors with old scoring,
lexical retrieval without contracts, and full Enhanced. Native users cannot select
experimental ablations.

| Entries | Standard Recall@3 | Enhanced Recall@3 | Enhanced false activation | Harmful sibling exposure |
| --- | --- | --- | --- | --- |
| 25 | 16/40 | 20/40 | 0/20 | 0/20 |
| 100 | 15/40 | 20/40 | 1/20 | 0/20 |
| 250 | 15/40 | 20/40 | 1/20 | 0/20 |
| 500 | 15/40 | 20/40 | 1/20 | 0/20 |

Enhanced Recall@3 has a 95% Wilson interval of 35.2–64.8%. At 500 entries:

| Condition | Positive Hit@1 | Positive Recall@3 | Positive abstention | Sibling hit | Harmful sibling | Missing-resource activation |
| --- | --- | --- | --- | --- | --- | --- |
| Standard | 14/40 | 15/40 | 24/40 | 4/20 | 4/20 | 10/20 |
| Descriptors + old scorer | 14/40 | 15/40 | 24/40 | 12/20 | 4/20 | 10/20 |
| Lexical, no contracts | 19/40 | 20/40 | 16/40 | 18/20 | 2/20 | 10/20 |
| Enhanced | 19/40 | 20/40 | 16/40 | 18/20 | 0/20 | 0/20 |

Per-family positive recall: debugging 2/3, diagram 3/3, document 1/4,
editing 1/3, geography 1/3, meetings 3/3, papers 3/3, PDF 0/3, planning 1/3,
presentation 0/3, QA 3/3, review 1/3, spreadsheet 1/3. The held-out subset without
literal skill names has 12/31 hits and 15/31 abstentions. No-skill false activation
is q098. Positive misses: q004, q006, q010, q012, q014, q016, q018, q020, q022,
q024, q032, q034, q038, q042, q046, q048, q064, q070, q072, q080.
Examples include an editable financial spreadsheet request (q004) and a formatted
DOCX letter request (q010); q098 is a request to do nothing yet. These are material
misses, not merely unusual phrasing or tasks outside the stated goal.

**HOLD:** 50% recall fails 95%; 40% positive abstention fails 5%; 5% no-skill
activation at larger sizes fails 2%. Improved relative scores do not satisfy these
absolute gates. Standard stays default; no release or default promotion is claimed.

The synthetic groups repeat templates across splits, limiting independence.
Held-out syntax cases do not balance valid explicit invocations; separate parser
regressions cover those. A fresh independently reviewed holdout is required for
strong generalization claims. Real skill sources were not enriched: optional
descriptors in this experiment belong to synthetic fixture procedures. No thresholds
were tuned against held-out predictions. Embeddings are optional and unimplemented;
full-body retrieval is excluded by the current contract.

## Acceptance and unresolved items

| Requirement | Current evidence and limitation |
| --- | --- |
| R1 | Literal request and existing routing regressions pass; parsing preserves submitted text. |
| R2 | Bounds, malformed/stale sidecars and local/global shadowing tests pass; freshness stamp is not full-body attestation. |
| R3 | Three insertion orders agree; live policy filtering, disabling/archive/delete/source replacement and cache tests pass. A 500-entry notice states that additional files may not be indexed. |
| R4 | Typed action/artifact/effect/resource fixtures and role-flip matrix pass; absent typed resource facts remain unknown. |
| R5 | Outcomes and bounded refinement helper tested; quality gates fail. Native mid-turn steering does not invoke a second routing search. |
| R6 | Existing loading/chunk tests pass on PR-1 baseline; combined completed score-floor regression awaits GLM PR-2/PR-3. |
| R7 | Shared AgentLoop/VRO/ReAct composition plus real native Settings/ACP controls pass; no live-provider/all-platform runtime claim. |
| R8 | Standard default, opt-in preview, save/discard/keep/restart and explicit ACP persistence tests pass. |
| R9 | Frozen matrix completed; promotion HOLD for the failures above. |
| R10 | Local cost budgets pass; embedding and downstream task-success experiments unexecuted. |
| R11 | All 488 library files match pre-work SHA-256 inventories; no source changes or automatic disabling. |

The implementation cannot be called full PRD acceptance. Remaining work includes
quality improvement under the library-preservation constraints, native transition
refinement integration if needed for in-flight steering, independent evaluation,
and combined score-floor acceptance. GLM's pending work does not explain away the
quality failures. User testing should use an isolated build; this branch has not
replaced the installed TUI.

## Preservation and readiness

All 488 `skills/` files match `/tmp/routing-library-before.json`. The SHA-256 of
the sorted compact JSON path/hash inventory is
`cadf102d871a171adf7aebb885a26d0ccc124c41b5543dec90d48dc60e192ab0`.
The prediction receipt SHA-256 is
`85a41caf375bc3393e651cf856983eb309b62d4a19bb42f831f5eec1c95d907c`. The five GLM-owned
baseline files match `/tmp/vesper-routing-glm-baseline.json`, and `rank_chunks` is
byte-identical to `5cf5835`. The original checkout is untouched. This branch's
origin is a local checkout, not a publishing remote; nothing was pushed. No version,
release, registry or installation change was made.

## DOX

Memory, harness, domain, both host contracts and native terminal-test ownership
were updated. Parent indexes retain their existing boundaries; no skill-library
contracts or files were rewritten. Root/global release, permission, provider and
body-loading contracts remain unchanged. The owning PRD and evidence index link
this report. Source inventories were rechecked. JSON parses and the summary reproduces the
recorded metrics; changed Markdown links and final whitespace checks are checked
at closeout. No child index boundary changed.
