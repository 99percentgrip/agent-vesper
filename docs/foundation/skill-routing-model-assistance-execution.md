# Model-assisted skill selection execution

Date: 2026-09-13. Status: IMPLEMENTED, offline verification passed; routing-quality adoption HOLD.
Owner: [routing PRD](../skill-routing-quality-prd.md) and [approved amendment](../skill-routing-model-assistance-proposal.md).
Independent review: [evaluation record](skill-routing-independent-execution.md).

## Objective and authorization

Select the right skills from natural task descriptions while preserving the complete
library and the existing permission/loading boundary. Alex approved the additional
opt-in configured-provider call. Alex separately approved one bounded live evaluation: at most 205 tool-free
Z.ai GLM-5.3 calls, each capped at 20 seconds and 1,024 requested output tokens.
That single pass finished successfully. Its independently scored results keep
fallback separate from model success. Standard remains the default.

Work is isolated in `/tmp/vesper-routing-quality`, branch `feat/skill-routing-quality`,
on `59fd515419fa0da6b644a23725983238dfcf4d64`. GLM's original workspace remains at
score-floor PR-1 `5cf5835f78d8e779443188c3490172d7736a589a`. No merge, release,
version bump, installation, or modification of GLM's protected work is performed.

## Implementation and files

- `vesper-memory/src/model_routing.rs`: bounded metadata offers, private prepared
  snapshots, strict result parsing and shortlist membership validation. The existing
  orchestrator prepares before loading bodies, then rebuilds eligibility to complete.
  Selected IDs never become an explicit `/skill` request or permission grant.
- `vesper-memory/src/routing_quality.rs`, `skills.rs`: default-off preference and
  store-root identity binding. Catalog snapshots include task, settings, current
  metadata/freshness, platform and available tools. Descriptor effects cannot be
  understated by permissive legacy metadata. The chunk ranker is unchanged.
- `vesper-harness/src/skill_model_selector.rs`: shared tool-free native AgentLoop,
  one iteration, 20-second provider deadline, 1024 requested output tokens, strict
  4096-byte response and at most 8192 stream events. An owner-scope guard cancels
  even if a provider retains its signal. Parent cancellation is observed during
  stalled session creation. Report only IDs, bounded reason, elapsed time and
  actual provider-reported usage; absent usage remains unavailable.
- `vesper-agent/src/agent_loop.rs`: optional advisory output-token and text-only
  stream bounds. Ordinary coding turns keep their existing defaults. Advisory
  streams reject tool/non-text events before aggregation and count hidden reasoning
  bytes without retaining or exposing that reasoning.
- Shared domain parser and harness Settings: `save model-assistance on|off`.
  Enabling selects Enhanced; Standard deactivates assistance. TUI Skills uses its
  existing grouped draft/save/discard/keep-editing flow and explains added usage.
- Both apps: TUI direct/VRO/ReAct resolve pending selection inside their background
  turns; ACP resolves before its direct/VRO dispatch and registers cancellation.
  TUI passes original task text to selection, not expanded `@file`/`@diff` content;
  expanded references remain available to the normal coding turn. Selection notices
  use the existing FIFO and cannot terminate a running turn. Original message
  contents remain restored before persistence.
- `apps/agent-vesper-tui/examples/skill_routing_eval.rs`: non-shipped diagnostic,
  dry-run by default. Explicit live mode is capped at the frozen 205 cases, uses
  Z.ai GLM-5.3/coding/enabled reasoning through the actual adapter, writes only a
  new requested result file, and preserves library hashes. Unsupported corpus
  resource constraints remain labelled unsupported; these inspected cases are
  regression evidence, never a new holdout.

## Independent findings and repairs

The reviewer found and prompted repairs for understated descriptor effects,
assisted index failure bypass, cross-store prepared responses, reference-expanded
file disclosure, continuation after cancellation, provider-retained cancellation
signals, and forbidden stream-object accumulation. Boundary, fixture-provider,
reference-canary, owner-drop and deadline tests cover the corresponding shared
paths. Host-specific wiring is also inspected and compiled; no live-model quality
claim is derived from this static review.

Full-body cryptographic attestation is not claimed. Preparation reads only bounded
catalog metadata and source size/mtime freshness; deliberately preserved timestamps
and changes to unread suffix/chunk bytes are outside that snapshot. Current loaders
still enforce current selected-body/chunk validity and budgets. This distinction
preserves the approved metadata-only boundary rather than hashing whole libraries.

## Methods and exact evidence

Commands use `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`
in the isolated checkout. No foundation command uses a live provider.

- `cargo test -p vesper-memory --test model_routing --offline`: **5 passed**.
  Strict JSON/IDs, opt-in revocation, changed task/store/source, isolated bodies,
  input-overflow fallback and stricter descriptor effects.
- `cargo test -p vesper-harness --all-features --test context_paging_composition --test skill_routing_settings --offline`:
  **14 composition + 5 Settings tests passed**, including no executable tools,
  no body in selection requests, 20-second stalled-provider deadline, oversized
  output, owner-drop cancellation and file-reference canary separation.
- `cargo check -p agent-vesper-tui -p agent-vesper-acp --all-features --offline`:
  both hosts compiled after reference/cancellation wiring.
- `cargo run -p agent-vesper-tui --all-features --example skill_routing_eval --offline`:
  `DRY RUN; zero provider calls: 205 cases, 205 prepared, 488 source files unchanged`.
  Prepared does not mean a nonempty shortlist or successful model decision.
- First `cargo xtask verify`: **2301 workspace tests passed, 35 ignored**, then
  required targeted/doc/architecture/fixture checks and **23 exact acceptance cases**.
  That run predates final review fixes and is retained as prior evidence only.
- Final `cargo xtask verify`: **2,305 workspace tests passed, 35 ignored**, followed
  by the required targeted/doc/architecture/fixture checks and **23 exact acceptance
  cases passed**, exit 0. This run includes the final review fixes.
- `cargo +1.88.0 check --workspace --all-targets --all-features --locked --offline`:
  passed. This is MSRV compilation, not an MSRV runtime-test claim.
- Native `skill_routing_pty.py`: **PASS**, draft/discard/keep/save/restart and
  source-library preservation, with the model-assistance toggle enabled.

The [source manifest and receipt hashes](skill-routing-model-verification.json) bind
693 source/fixture/build files to this verification; the uncommitted source digest
is `6b3b48ff5bbf495c706320282adc9fda2c79203231fae6cb5c86a78a85d10201`.
All 488 library files and five GLM-protected files match their pre-work hashes;
`rank_chunks` is byte-identical to the PR-1 baseline.

Logs are `/tmp/routing-model-{memory-tests,provider-tests,drop,deadline,hosts,eval-dry,verify}.log`.
The earlier complete verification log is `/tmp/routing-model-verify-before-review.log`.

### Local cost and current lexical regression

`cargo test -p vesper-memory --all-features --test routing_quality_eval --test routing_quality_perf --offline -- --nocapture`
completed successfully as measurement. On Linux x86_64, AMD Ryzen AI 9 465,
unoptimized debug profile, 500 synthetic duplicate-topic entries: index cold build **51.572 ms**, warm p95
**3.154 ms**; complete store routing cold **94.848 ms**, warm p95 **23.619 ms**
(10 warm-ups and 100 timed queries each). This excludes provider latency.
The [four-size lexical receipt](skill-routing-current-quality-results.json) still
fails positive recall; at 500 entries it records **27/40**. No model-assisted
quality conclusion is derived from that lexical measurement.

## Authorized live evaluation — completed

The launcher exited **0** after all **205 cases** and confirmed all **488 library
files unchanged**. [Raw receipt](skill-routing-model-live-receipt.jsonl),
[independent scoring/report](skill-routing-model-live-evaluation.md),
[machine-readable results](skill-routing-model-live-results.json) and
[reusable scorer](skill-routing-model-live-summary.py) retain every case.
No second live pass was run.

| Measurement | Standard on matched cases | Model-assisted final route |
| --- | ---: | ---: |
| Expected-skill Recall@3, positives | 60/95 | 89/95 |
| Strict positive selection (no extras) | 39/95 | 84/95 |
| Unnecessary activation, no-skill cases | 5/60 | 0/60 |
| Forbidden sibling activation | 10/30 | 2/30 |
| Strict sibling selection | 9/30 | 26/30 |

These are final native results, including fallback; they are not all credited to
the model. There were **180 validated model decisions**, **5 provider-error lexical
fallbacks**, and **20 deterministic empty-shortlist decisions without a provider
call**. Provider-path attempts therefore total 185; exact HTTP attempts for errors
are unavailable. Among the 95 positives, 91 had actual model decisions and four
used fallback; 81 model decisions were strictly correct, with three further strict
successes supplied by fallback.

Six positive targets were absent because current policy excludes four platform-
incompatible Apple skills on Linux and two external-action skills. All 95 remain
in the authoritative denominator. The separately labelled eligible-only diagnostic
is **89/89 recall, 84/89 strict selection**; it does not replace the all-case gate.
Five eligible positives selected an unwanted additional skill. Two sibling cases
selected a forbidden alternative. All 12 resource cases remain unsupported by this
fixture's native context mapping, so none receives supported-policy-pass credit.

Provider-path latency: median **3.184 s**, p95 **10.010 s**, maximum **13.254 s**.
The 180 usage receipts report **177,715 total units** (136,011 input, 41,704 output).
Five errors have unavailable usage; their cost is not assumed zero. Reasoning and
cached-input fields overlap their parent counters and are not added again. No
monetary charge is inferred from coding-plan usage. The independent report includes
95% Wilson intervals with the limitations of a curated, inspected corpus.

Raw receipt SHA-256:
`9558a2f69bee358f673e0286acbd39c5ce3fd62c506f310be32db12de73b8ccf`.
Completion log: `/tmp/routing-model-live-20260913.log`, SHA-256
`2afeede0f125d10c9c446a64fcf1053b2ffa3371d69fac4d4ee319d08ffe699a`.

The bounded live work unit is complete. Overall quality promotion remains **HOLD**:
positive recall is below the all-case 95% threshold and sibling exposure exceeds
2%. This single inspected-corpus, Linux/GLM-5.3 run does not establish fresh held-out,
all-library-size, other-provider, other-platform or downstream task effectiveness.

## Deviations and unresolved acceptance

Staged `git diff --check` flags only the original upstream license comment
whitespace in `routing-verbs.txt`. The byte-reproducible licensed asset is retained
unchanged; the same check excluding that single data asset passes.

Provider/parse failures and oversized input use an explicit lexical fallback;
index failures, stale decisions, unreadable settings and cancellation withhold
model-selected activation. Fallback is never counted as successful model selection.
The 50-ms local retrieval budget excludes separately reported provider latency.
No native refinement on in-flight steering is claimed: the existing bounded
transition helper is not itself a host-integrated mid-turn rerouting system.

The independent first-run failures remain in their immutable receipt. Lexical
follow-ups and embedding experiments have not passed the full positive/no-skill/
sibling/no-name gates. Scripted-provider tests prove mechanics, not interpretation
quality. The approved live regression is now measured; fresh independent adoption
evidence, model-assisted results at all required library sizes and scope-appropriate
platform evidence remain outstanding. GLM PR-2/PR-3
integration still requires their completed commit; this work does not overwrite it.

## DOX and readiness

Owning memory, harness, agent, domain, app and native-test contracts were updated.
Changed Markdown targets and JSON/JSONL receipts validate; independent live scoring
reproduces the retained result byte-for-byte. The final source manifest still
matches after evaluation and documentation closeout.
The new explicit-example boundary has its own indexed AGENTS.md. Documentation
owners, evidence index and PRD link this report. Root/skills/chunk ownership remains
unchanged because existing boundaries and the library are preserved.

Readiness is an opt-in implementation with passing offline verification and a
completed live regression, not quality promotion, release readiness or a claim
that the overall routing goal is complete.
