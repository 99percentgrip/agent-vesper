# Independent skill-routing evaluation: frozen corpus

Status: **corpus frozen; first native measurement independently audited; quality
failures and unsupported compatibility cases remain**. The original construction
record below describes the pre-execution freeze; the addendum records the later
authorized review.

Subsequent status (2026-09-13): the separately authorized 205-case live run is
complete and independently scored in the [live evaluation report](skill-routing-model-live-evaluation.md).
Earlier review-time references to pending live evaluation below are historical;
the completed run retains failures and is inspected-corpus regression evidence.

## Objective

Provide a fresh held-out evaluation of automatic task-to-skill selection against
the actual bounded catalog, without using previous examples, predictions, failure
prompts, or routing implementation to choose labels. Preserve all skill sources.
This record is corpus-construction evidence, not an adoption verdict.

## Independence and methods

- A separately delegated evaluator authored cases in the isolated
  `/tmp/vesper-routing-quality` checkout. It read the root, documentation, and
  foundation `AGENTS.md` contracts, then only the supplied bounded catalog.
- The sole labeling source was `/tmp/routing-bounded-metadata.json`: 95 real
  entries containing slug, name, description, tags, triggers, and extensions.
  No skill body, prior routing corpus, prior label, prediction, failure prompt,
  implementation, or PRD case was inspected before freezing. The mandated
  `AGENTS.md` chain names earlier reports but does not expose their cases.
- Tasks were individually written from capabilities: realistic requests,
  execution/artifact-specific sibling distinctions, straightforward conversation
  and controls, and explicit execution incompatibilities. The author script
  contains literal tasks; it does not generate paraphrases from token templates.
- The author is an AI evaluator, not a human panel. Independence means separation
  from implementation and prior predictions during authorship, not an external
  organization, different model family, or human inter-annotator agreement.
- Every case belongs to `independent_holdout`; no development split is drawn from
  these cases. Labels were frozen and their hash delivered to the implementing
  agent before prediction execution. Any later tuning against their results makes
  subsequent runs diagnostic, not a new unseen-holdout measurement.

## Files and exact freeze evidence

- [Corpus](skill-routing-independent-corpus.json): 205 unique prompts and IDs.
- [Digest](skill-routing-independent-corpus.sha256): SHA-256 of exact UTF-8 JSON
  bytes, including the final newline:
  `79ef89a4934a0e2c61aa08d4e2de6a3efd0272c18f1ee3e9507271d5c6111275`.
- [Authoring source](skill-routing-independent-author.py): deterministic creation
  and structural assertions, using only the supplied catalog as input.
- This report records method, validation, and limitations. Prediction receipts and
  implementation reports are separate artifacts owned by the coordinating agent.
- Catalog SHA-256:
  `96b28ecb189cbef73ef66b70a65571c3baf46d92b7320d1b2e8ccc8774991482`.
- Checkout HEAD at corpus validation:
  `59fd515419fa0da6b644a23725983238dfcf4d64`.

| Kind | Cases | Label intent |
| --- | ---: | --- |
| Positive | 95 | One clear task per real catalog skill |
| No skill | 60 | Conversation, control, or self-contained general answer |
| Sibling | 30 | Requested workflow distinguished from plausible wrong siblings |
| Resource/permission | 12 | Execution blocked by explicit unavailable capability or denied action |
| Ambiguous | 8 | Missing operation, destination, medium, or delegate |
| Total | 205 | 197 clear and 8 ambiguous |

The 95 positive cases cover all 95 catalog IDs. This exceeds the requested 80
positive, 60 no-skill, and 40 combined sibling/resource targets. Cases were not
added or relabeled in response to predictions.

## Scoring contract

Each case has `id`, `split`, `kind`, `clarity`, `prompt`, `expected_skill_ids`,
`acceptable_abstention`, `forbidden_skill_ids`, `context`, and `rationale`.
`context` contains `unavailable_tools` and `denied_permissions` arrays.

- `expected_skill_ids` is the set of acceptable selected IDs, not a request to
  activate every listed alternative. Selections must contain at most three
  distinct catalog IDs. Any selected ID outside that set fails the strict case.
- Clear positive and sibling cases require a nonempty subset of their acceptable
  IDs. Abstention fails these cases. Sibling cases also record specifically
  prohibited near neighbors.
- No-skill and resource cases require an empty selection. The empty result is the
  required label, not optional abstention on an uncertain positive.
- Only the eight explicitly ambiguous cases allow either an empty selection or
  a nonempty subset of the acceptable IDs. Report their results separately; do
  not improve clear-task accuracy by folding ambiguity into its denominator.
- A selected forbidden ID always fails. Task-directed routing is evaluated;
  always-on repository instructions are not additional skill activations.
- Report clear positive recall, no-skill false activation, sibling correctness,
  permission/resource compatibility, ambiguous outcomes, and the complete
  per-case predictions separately. A top-1 correct answer with extra unrelated
  activations does not satisfy the strict selection criterion.

Resource labels are conditional on the supplied context. Tool names can denote
external executors such as `imsg`, `twozero`, or a platform capability, rather
than native registered tool IDs. Permission labels describe denied actions such
as external writes or installation. A runner must record the exact native
mapping it actually enforced. If the native API or descriptors cannot express a
case's constraint, mark it **unsupported/unexecuted**, retain it in coverage, and
report it separately. Do not silently score it as unrestricted semantic routing,
drop it from the requested coverage, or count an incidental empty prediction as
proof of native permission enforcement. The cases do not prohibit discussing
these tools generally: their prompts request execution and an explicit blocked
result when execution cannot run.

## Commands and verification

Run from the isolated checkout:

```sh
python3 docs/foundation/skill-routing-independent-author.py
```

The authoring command exited 0 and printed:

```json
{"sha256":"79ef89a4934a0e2c61aa08d4e2de6a3efd0272c18f1ee3e9507271d5c6111275","cases":205,"counts":{"positive":95,"no_skill":60,"sibling":30,"resource":12,"ambiguous":8},"distinct_positive_skills":95}
```

An independent JSON parse/hash/count check confirmed 205 unique IDs, 205 unique
prompts, 197 clear cases, and maximum three acceptable IDs. The script asserts
that every label and forbidden ID exists in the actual catalog, expected and
forbidden sets are disjoint, all minimum cohort sizes are met, and optional
abstention occurs only on ambiguous cases. A Python `compile` syntax check passed
without generating bytecode files. No published JSON Schema existed for this
new corpus, so these are explicit structural checks rather than a claim of
validation against an existing schema.

```sh
git diff --exit-code -- skills
git rev-parse HEAD
```

The tracked seed-library diff was empty (exit 0). This evaluator performed no
write outside its four `skill-routing-independent-*` artifacts and did not read
or modify Alex's installed library or original working tree. Full installed
library before/after hashing belongs to the coordinating agent; a tracked Git
diff alone does not establish installed-library byte invariance.

## Deviations and unresolved items

- Routing execution and all performance/quality/adoption thresholds remain
  unexecuted by this evaluator. No passing routing result is asserted here.
- The corpus is intentionally balanced for capability coverage, not sampled
  from production traffic. Many positive cases identify a product, artifact
  format, or executor because that distinguishes real catalog siblings. It
  should not be represented as a measurement of entirely implicit-intent recall.
- Labels use the bounded catalog, not deep source-body interpretation. Some
  broad/general skills can assist incidentally with another skill's task; the
  strict labels evaluate requested workflow relevance, not every potentially
  useful background instruction. No second human adjudicator has reviewed them.
- No end-to-end provider interaction, native host session, wall-clock latency
  benchmark, Windows/macOS run, or permission side effect was performed.
- The 12 incompatibility cases need native context mapping or an explicit
  unsupported result. The case count alone is not compatibility evidence.
- The generator requires the exact catalog input at the documented temporary
  path. The checked-in corpus and digest are self-contained for execution;
  reproduction requires restoring that exact catalog input.

## DOX and readiness effect

The root → `docs/AGENTS.md` → `docs/foundation/AGENTS.md` chain was read before
creation and checked against the changed artifact scope at closeout. Shared
`AGENTS.md`, evidence index, and PRD were intentionally left unchanged because
the coordinating agent owns their integration and links for this work unit.
No new subtree or child index is introduced.

The corpus is ready for a first frozen-label evaluation. It adds independent
measurement material; it does not by itself change production readiness,
authorize skill-source changes, or establish any routing adoption gate.

## Audit addendum: first native measurement

After the frozen digest was delivered, the coordinating agent authorized this
evaluator to inspect the new runner, its native implementation, and predictions.
The evaluator then inspected
`crates/vesper-memory/tests/routing_quality_independent.rs`, relevant portions of
`src/skill_orchestrator.rs` and `src/routing_quality.rs` in that crate, the baseline
tool registration in `tests/routing_quality_eval.rs`, and relevant source
frontmatter. All such inspection happened **after** corpus freeze. No labels or
skill source bytes were edited. From this point onward, architecture changes
informed by these results are evaluated against a known diagnostic corpus; a
later rerun must not be described as an unseen-holdout success.

### Receipt and scoring audit

The runner's first result was read from
`/tmp/routing-independent-native-results.json` and preserved byte-for-byte as
[first native results](skill-routing-independent-first-results.json), 177,737
bytes, SHA-256
`ade410b7b0394cb3a8a47d5484e290c6e8c5c1184836b46b0abdbdb4b5ac2aa8`.
It contains 410 predictions: 205 cases for each of Standard and Enhanced.

The evaluator initially found a scoring defect: the first runner expression
required a positive hit or optional ambiguous abstention, so it incorrectly
failed empty selections on every required no-skill/resource label. This was
reported before receipt inspection. The coordinating agent corrected the empty
expected-set branch before the preserved run. Independent recomputation of
selection limits, uniqueness, expected membership, forbidden membership, and
abstention semantics found **zero label-satisfaction mismatches** across all 410
records in the corrected receipt.

The nine registered tool IDs exactly match the existing native offline evaluation
fixture. They describe that fixture, not discovery of installed external CLIs.
The runner uses Linux for every case and default routing options except mode.
It correctly reports every nonempty `denied_permissions` array as unsupported,
and reports unavailable executors absent from its tool registry as unsupported.
All 12 resource cases therefore remain **unsupported**, in both modes, and none
contributes a supported pass. Standard happened to abstain on four resource
prompts; this is not evidence of permission enforcement. Enhanced activated at
least one skill on all 12 and activated the specifically forbidden target on
nine. Neither behavior establishes a supported compatibility measurement.

Remaining runner limitations:

- The corpus digest freezes prompts and labels; it does not fingerprint the
  loaded skill library, bounded metadata, descriptors, or native executable.
  Separate library/build identity evidence is needed for reproducibility.
- The assertion that at most three selections exist is a structural check. A
  green test records measurements even when quality is poor; it does not assert
  any adoption threshold.
- `context_supported` means the supplied context fields were representable. It
  does **not** mean the expected target is eligible on Linux. Eligibility-aware
  diagnostics must additionally inspect target rejections.
- The receipt records expected-target rejections but not all candidate scores
  and rejection reasons. A missing target with no eligibility rejection can be
  identified as a selection miss; its precise score-stage cause needs further
  tracing rather than speculation.

### Full-scope results

These are the frozen denominators. No unavailable target has been relabeled or
removed from them. A hit means at least one expected skill was selected; strict
success additionally excludes every unexpected activation.

| Cohort | Standard | Enhanced |
| --- | ---: | ---: |
| Positive hit | 60/95 | 84/95 |
| Positive strict success | 39/95 | 67/95 |
| Positive abstentions | 33/95 | 5/95 |
| No-skill false activation | 5/60 | 16/60 |
| No-skill required abstention | 55/60 | 44/60 |
| Sibling hit | 15/30 | 23/30 |
| Sibling strict success | 9/30 | 9/30 |
| Sibling forbidden activation | 10/30 | 20/30 |
| Ambiguous label satisfied | 8/8 | 6/8 |
| Unsupported resource context | 12/12 | 12/12 |
| Supported resource passes | 0 | 0 |

### Eligibility-aware diagnostics

Both modes reject the same six positive targets before ranking: four for
`unsupported platform linux` (`imessage`, `findmy`, `apple-reminders`,
`apple-notes`) and two for `external side effect not explicitly requested`
(`huggingface-hub`, `github-repo-management`). Four sibling targets encounter the
same categories: two Apple platform restrictions and two external-request gates.
There are no expected-target `user-only`, `required tool unavailable`, or
`isolated worker unavailable` rejections in this receipt. Those remain distinct
taxonomy categories, not observed explanations for these misses.

| Diagnostic cohort | Standard hit / strict | Enhanced hit / strict |
| --- | ---: | ---: |
| 89 positives without recorded target eligibility rejection | 60/89; 39/89 | 84/89; 67/89 |
| 26 siblings without recorded target eligibility rejection | 15/26; 9/26 | 23/26; 9/26 |

These denominators help distinguish ranking from eligibility, and do not replace
the full-scope results. Enhanced has five eligible positive abstentions; all
other positive misses are the six recorded eligibility blocks. It nonetheless
activates unexpected alternatives in **all six** blocked-positive cases and all
four blocked-sibling cases. Target blocking can therefore be correct while the
resulting fallback selection is still wrong.

### Structural failure categories

The following diagnoses are grounded in recorded prompts and selections. They
describe general routing boundaries, not proposed per-case vocabulary patches.

1. **Conversation and instructions versus executable work.** Definitions,
   background information, and control instructions receive specialist routes.
   Cases `independent-134` (definition of a pull request), `independent-146`
   (owning a bulb without a task), and `independent-122` (do not make changes)
   demonstrate distinct forms of this boundary failure.
2. **Generic lexical overlap mistaken for specialized intent.** Simple language
   tasks activate Word-document editing, while small numeric or conversational
   requests activate harness or project-verification workflows. Examples include
   `independent-102`, `independent-127`, and `independent-149`. The presence of an
   imperative request alone does not establish a catalog workflow.
3. **Negated alternatives treated as positive evidence.** Explicitly prohibited
   formats, applications, and operations still activate: browser presentation
   versus PowerPoint (`independent-162`), one inference runtime versus another
   (`independent-173`), one note store versus another (`independent-179`), and
   audio analysis versus composition (`independent-183`). Twenty Enhanced sibling
   cases activate a specifically forbidden ID.
4. **Domain relevance without operation or artifact discrimination.** Related
   skills compose even when their operations are not requested: structural PDF
   manipulation attracts OCR/text editing; one language's debugger attracts the
   other; supplied meeting material attracts source-acquisition workflows.
   Cases `independent-033`, `independent-026`, and `independent-156` illustrate
   this. Enhanced positive hit gains exceed strict-success gains because 17
   positive hits also select an unexpected skill.
5. **Fallback after expected-target rejection.** Missing platform support does
   not make a different note store, unrelated cloud service, or document editor
   satisfy the original task. Enhanced fallback activations on every blocked
   positive are measured separately from the legitimate platform rejection.
6. **Coarse skill-level external-effect eligibility.** The target for a dataset
   download is blocked by a broad external-effect classification, while explicit
   repository creation/forking is not recognized by the action gate. These are
   operation/eligibility-model problems, not evidence that their semantic labels
   are wrong or that another selected skill meets the task.
7. **Request recognition misses contextual lead-ins.** All five Enhanced
   eligible positive misses abstain, including tasks that establish the artifact
   or situation before asking for an action. The receipt establishes the misses;
   it does not independently prove which parser or score threshold caused each.

### Audit verification and readiness

Read-only Python checks joined predictions to frozen case IDs, recomputed all
strict labels, counted rejection reasons, and calculated both full-scope and
eligibility-aware totals. JSON parsing and SHA-256 verification passed. This
evaluator did not run the Rust test itself; the coordinating agent owns its
command receipt. No production or shared documentation files were changed by
this evaluator. The new preserved-results link belongs to this report's existing
ownership scope; shared index/PRD/DOX integration remains with the coordinator.

The first run demonstrates improved positive recall alongside substantially
worse no-skill and forbidden-sibling behavior. It does not establish acceptable
automatic routing or supported resource/permission enforcement. Any repair
informed by this audit must retain these first results and frozen labels, report
subsequent runs as diagnostic, and obtain genuinely fresh independent evidence
before making an unseen-generalization claim.

## Review addendum: bounded negation normalization

The coordinator requested a subsequent read-only review of `relevance_text` in
`crates/vesper-memory/src/routing_quality.rs` and
`excluded_alternatives_are_not_positive_retrieval_evidence` in
`crates/vesper-memory/tests/routing_quality.rs`. This review used new synthetic
wording, not additional relabeling of the independent corpus. No production file
or frozen case was edited by the evaluator.

The reviewed version lowercases the prompt, globally removes `not only` and
`not just`, replaces ` but ` with a clause separator, splits on commas,
semicolons, newlines, periods, exclamation marks, and question marks, then
truncates each clause at its first supported negation marker. It correctly
retains the original prompt for contract checks and the provider, and computes
request/action features from the relevance view. The existing synthetic
regression covers simple comma negation, `rather than`, semicolon-separated
commands, contrastive `but`, and a wholly prohibited request.

### Reproduced text transformations and gaps

A read-only Python reproduction of those string operations produced the
following transformations. These are transformation-level reproductions, not
claims of an executed native end-to-end selection test.

| Input | Relevance text observed | Finding |
| --- | --- | --- |
| `Do not use quartz, velvet, or cobalt workflows.` | `velvet or cobalt workflows` | Comma resets resurrect a prohibited list |
| `Inspect amber records; do not open private.cobalt.` | `inspect amber records  cobalt` | A period inside an identifier resurrects forbidden content |
| `Do not inspect quartz records` followed by newline and `and cobalt records.` | `and cobalt records` | Line wrapping loses the ongoing prohibition |
| `Inspect anything but cobalt records.` | `inspect anything cobalt records` | Exception `but` is incorrectly treated as affirmative contrast |
| `Inspect quartz—not cobalt.` | `inspect quartz—not cobalt` | Space-only marker detection misses dash-separated negation |
| `Without velvet processing inspect quartz records.` | empty | Prefix adjunct truncates the later affirmative request |
| `Inspect quartz records, not cobalt: inspect amber records instead.` | `inspect quartz records` | Truncation discards an affirmative clause after the exclusion |

The final two examples demonstrate conservative false-negative risks; they do
not justify inventing a parser that claims to understand arbitrary clause scope.
An explicitly documented abstention for ambiguous unsupported grammar is safer
than resurrecting excluded alternatives. Likewise, `never omit` and `not
without` are not ordinary negative-topic exclusions. Blanket treatment loses
required positive material. Quoted text and filenames need lexical boundaries
that do not reinterpret their contents as routing instructions.

An initial review message suggested `Do not only inspect...` as a prohibited
target example. That suggestion was withdrawn: this construction can request
inspection plus additional work, so it is not an unambiguous exclusion test.
Local handling of `not only`/`not just` is still preferable to global rewriting,
but this review does not claim that example proves a wrong activation.

### Suggested synthetic regression matrix

Use arbitrary IDs with disjoint descriptors, for example `alpha` = quartz
microscopy, `beta` = cobalt metallurgy, `gamma` = velvet weaving, and `delta` =
amber indexing. Assert candidate/anchor presence, not merely top-1 order, so a
wrong sibling cannot hide behind a correct first match.

- Prohibited noun list: `Do not use quartz, velvet, or cobalt workflows.` No
  candidate should acquire positive evidence from those names.
- Prohibited verb list: `Do not grind quartz, polish cobalt, or weave velvet.`
  Later imperative-looking words must not end the list's negative scope.
- Wrapped prohibition: the two-line quartz/cobalt example above must not
  acquire cobalt evidence solely because of the newline.
- Identifier punctuation: the private.cobalt example must retain amber evidence
  while suppressing cobalt evidence from the prohibited filename.
- Exception language: `Inspect anything but cobalt records.` must not acquire
  cobalt evidence; an unspecified permitted target may appropriately abstain.
- Unicode boundary: `Inspect quartz—not cobalt.` should retain quartz evidence
  and suppress cobalt evidence.
- Positive local exception: `Inspect not only quartz but also cobalt records.`
  should preserve both affirmative topics.
- Explicit affirmative reset: `Do not inspect cobalt records; inspect quartz
  records instead.` should preserve quartz and suppress cobalt.
- Ambiguous prefix scope: `Without velvet processing inspect quartz records.`
  may conservatively abstain, but must never activate velvet from the exclusion.

### Bounded correction guidance and limitations

Persisting negative scope through list punctuation, distinguishing identifier
periods from sentence boundaries, and treating local `not only` constructions
without global substitution address general defects. However, a comma followed
by a dictionary-recognized imperative cannot by itself establish affirmative
scope: the prohibited verb-list test above is a counterexample. Explicit
contrast or a clear new affirmative sentence provides stronger evidence.
`anything but` is exclusion, while `but inspect` can introduce an affirmative
alternative; a blanket replacement cannot distinguish them.

A bounded scope-state scanner can improve these narrow cases, but unsupported
grammar should remain visible as uncertain rather than become fabricated intent.
The original provider request must remain unchanged, and these relevance
transformations must never grant tools, permissions, or side effects. The
coordinator owns implementation and native regression execution; this addendum
records review findings and proposed tests, not passing results for fixes.

## Review addendum: model-assisted two-phase memory boundary

This bounded review was requested after implementation of model assistance was
authorized; it does not authorize or execute a live provider call. The evaluator
re-read the root, crates, `vesper-memory`, documentation, and foundation DOX
contracts. Inspected inputs were `src/model_routing.rs`, the prepare/complete and
loader paths in `src/skill_orchestrator.rs`, revision/descriptor/chunk validation
in `src/skills.rs`, effective effects in `src/routing_quality.rs`, and the
[approved model-assistance proposal](../skill-routing-model-assistance-proposal.md).
All crate paths in this addendum are relative to `crates/vesper-memory/`.

### Authority checks observed

- `PreparedSkillSelection` snapshot fields are private and cannot be supplied
  through model JSON. Its public offer accessor exposes an immutable slice.
- `ModelSkillDecision` denies unknown fields; parsing enforces the response byte
  cap, known outcomes, at most three unique valid slugs, offered-ID membership,
  and consistency between the outcome and whether IDs are present.
- Completion rebuilds the eligible catalog from current source metadata.
  Selected model IDs enter a score map, never `explicit_skill` or bundle
  invocation. User-only skills remain excluded; ordinary tool, platform,
  invocation, effect/resource, conflict, and body/chunk gates remain in use.
- Switching off assistance or changing to a genuinely explicit request prevents
  the supplied model decision from entering the assisted automatic branch.
- Snapshot comparison binds the exact task hash, routing options, tool set,
  platform, and sorted catalog metadata/revisions/descriptors. Model JSON cannot
  replace any of those policy fields.
- Inline bodies reach the existing loader after the decision path; isolated
  entries retain empty main-context bodies and their existing worker boundary.

No direct conversion of a model-selected ID into explicit user authority was
found in the inspected normal completion path. This is a static review finding,
not a claim that all adversarial runtime tests have passed.

### Gaps and contract questions reported

**Offer effect understates stricter descriptors.** The reviewed `SkillOffer`
constructor derives `effect` from `metadata.risk`, while `RoutingIndex` uses the
descriptor's effect when one is present. Validation allows a descriptor to be
stricter than the metadata minimum. Thus a metadata-read-only skill with a valid
external-effect descriptor is enforced as external by retrieval but described
as read-only to the selecting model. Use the same effective effect in both
representations. Existing native enforcement still applies; this is misleading
selection metadata, not proof of an execution-permission bypass.

**Snapshot identity is weaker than a source snapshot.** The prepared object has
no store/root or resolved local/global source identity. `routing_revision`
combines a bounded-prefix FNV hash, file length, and modification time; it is
explicitly documented as a freshness stamp rather than content attestation.
Chunk-file contents and freshness are not included. Matching metadata and stamps
can therefore permit cross-store reuse, same-size suffix changes with preserved
mtime, or chunk changes between prepare and completion. Multi-byte UTF-8 makes
the suffix issue meaningful: a selected 24,000-character body can extend beyond
the 32,000-byte discovery prefix. This does not automatically grant additional
permissions, but it prevents a claim of exact source-byte snapshot binding.
Bind store/resolved source identity and appropriate chunk freshness, and state
the remaining limits. Do not read full isolated bodies merely to claim stronger
attestation: that would violate the separate isolation contract.

**Index-error fallback bypasses the normal decision-validation branch.** In the
reviewed implementation, index construction/cache failure invokes Standard
routing, which may load bodies without validating a supplied prepared decision.
The initial review flagged this against the memory DOX statement that no body is
loaded before a decision passes fresh checks. The approved proposal, however,
explicitly permits observable lexical fallback for oversized tasks and bounded
fallback for provider failures. Accordingly, this is a contract/failure-path
question, not an assertion that all lexical fallback is unauthorized. Either
fail closed for assisted index errors, or define and test an explicitly ordinary
lexical fallback that never credits model IDs or claims validated model
selection. Conversely, the reviewed oversized-task path returned an empty
Fallback rather than the proposal's specified lexical fallback; resolve that
behavior/documentation mismatch deliberately.

### Suggested offline acceptance tests

- Prepare using arbitrary eligible `alpha` plus a user-only `beta`; prove that
  no selected body/context appears yet and that choosing `beta` is rejected.
- Submit duplicate IDs, unknown IDs, an explicit-command-looking ID, four IDs,
  unknown JSON fields, oversized output, selected-with-empty, and
  no-skill-with-nonempty results; all must fail without body exposure.
- Prepare, then change the prompt, tools, platform, disabled settings, invocation
  policy, archive state, descriptor, or source revision. Completion must reject
  the old snapshot rather than use model selection as explicit authority.
- Add a stricter valid descriptor effect and verify the offer advertises that
  effective effect. Narrow host permissions afterward and verify completion
  rejects the stale decision.
- Prepare in store A and complete against store B with the same slug, metadata,
  length, and mtime; test the intended store-identity policy explicitly.
- Change a chunk after preparation, and change visible multibyte body content
  beyond the prefix while retaining size/mtime. Record what the revision scheme
  detects and preserve any documented attestation limitation.
- Exercise index failure and task/metadata overflow in both prepare and complete
  paths; assert the chosen fallback contract, trace outcome, and lack of model
  authority. A deterministic oversized index can exercise failure without live
  providers; an internal cache-failure fixture can cover the lock-error branch.
- Verify a valid isolated selection still exposes no isolated body or chunks in
  main context, and conflicting selections remain subject to the existing
  composer rather than bypassing it through model ordering.

No model-boundary tests were executed by this evaluator during this static
review; implementation and its offline acceptance command receipts remain with
the coordinator. No production, skill-library, or frozen-corpus file was changed
by the evaluator. These findings were sent while implementation was underway;
later fixes require their own current verification and must not erase the
reviewed limitations or be represented as live routing-quality evidence.

## Follow-up review: provider selector and native host composition

The coordinator requested review of implemented memory fixes and the shared
provider boundary while full verification was running. This pass inspected the
applicable application/harness DOX contracts and routing ownership sections,
`crates/vesper-harness/src/skill_model_selector.rs`, its four composition tests in
`tests/context_paging_composition.rs`, four memory tests in
`crates/vesper-memory/tests/model_routing.rs`, the revised memory implementation,
TUI direct/VRO/ReAct spawn paths, and ACP's shared pre-dispatch path. The evaluator
made only this report edit; no production, corpus, or skill-library edit and no
provider call was performed.

### Earlier findings addressed in inspected code

- Model offers now use the descriptor's effective effect when present, matching
  retrieval enforcement rather than understating it as legacy read-only risk.
- Prepared selections now bind a digest of the local and global store roots.
  `matches` checks that binding alongside task/options/tools/platform/catalog.
- Assisted index failure withholds selection rather than loading a Standard
  fallback before completing the model-validation branch.
- Oversized task/metadata preparation explicitly uses observable lexical
  fallback, matching the approved proposal's fallback behavior.

These fixes were confirmed by code inspection. The inspected memory tests cover
body-free preparation, strict decisions, changed settings/task/store/source,
explicit-only protection, isolated bodies, and the oversized-task fallback.
This evaluator did not independently execute their command receipts in this pass.

Full-body or chunk-content cryptographic attestation remains outside the approved
metadata-only snapshot scope. The existing bounded-prefix/size/mtime revision is
still a freshness stamp, not exact content attestation. That distinction should
remain documented, but it is **not** an additional blocker requiring full
isolated-body reads. Binding store roots fixes the previously missing cross-store
identity field within the approved scope.

### Provider boundary strengths

The selector constructs a fresh AgentLoop from the active configured provider,
replaces the system instructions with its bounded selection protocol, and uses
an empty executable registry, Plan/ReadOnly controls, one tool iteration, and
1024 requested output tokens. It sends no prior conversation or skill body from
the prepared object. It checks its context estimate before dispatch, requests no
tools, and accepts only a clean one-iteration textual result with no tool results.
The selection history is discarded. User-facing traces contain safe reason
codes, elapsed time, selected IDs, and provider-reported usage or unavailable.

A 20-second timeout surrounds provider session opening and execution. Text and
visible/summary reasoning progress count toward the 4096-byte cancellation
limit. Pending resolution rechecks current settings/catalog before dispatch and
rechecks settings and memory eligibility on completion. Both hosts use this
shared selector; TUI resolves it in the existing background task. These are
useful structural boundaries, not live model-quality evidence.

### Remaining findings reported during implementation

**1. TUI reference expansion enters the extra model request.**
`spawn_submitted_prompt` expands `@file`/other references, then supplies
`expanded` both to skill preparation and `PendingSkillRoute.prompt`. A small
expanded source file fitting the task byte cap is consequently sent in the
additional selection request. This violates the approved selection boundary of
current task plus bounded metadata, with no file contents or project scan.
Use the original unexpanded user task for model preparation/selection while
retaining expanded content for the ordinary coding turn. ACP's inspected path
uses textual task content before adding cognitive recall, so it avoids this
particular automatic expansion leak.

Suggested regression: submit an `@file` reference to a short file containing a
unique `FILE_CANARY`; inspect real captured selector and main requests. The
selector must contain the task/reference but not the file bytes, while the main
request retains the intended expansion. Run through direct, VRO, and ReAct entry
paths to verify the common preparation boundary.

**2. TUI can continue provider work after selector cancellation.**
All three inspected TUI background paths await `routed_skills.resolve` and
continue without an immediate cancellation check. Direct mode can then launch
MoA advisers using `worker.run_prompt` without the parent cancellation signal;
VRO/ReAct can enter acceptance/compaction work before reaching the normal loop's
cancellation check. ACP explicitly returns a cancelled response after resolving
the selector. Mirror an early terminal cancellation path in TUI, ensuring its
ordinary completion event still clears running state.

Suggested regression: hold the selector at a deterministic provider barrier,
cancel the TUI turn, release the barrier, and assert that main/adviser/compaction
provider counters remain zero. Cover all three TUI branches; a selector-only
pre-cancel test does not exercise this host continuation defect.

**3. Drop cancellation depends on the last shared signal reference.**
The reviewed `Drop for SelectionCancellation` is implemented on the object held
inside `Arc<dyn CancellationSignal>`. A provider may legitimately retain a clone
of that signal. Aborting/dropping the selector future then drops only its own
reference, so the shared object's destructor does not run and local cancellation
can remain false for retained provider work. Explicit timeout/completion paths
call local cancellation, but abrupt owner drop does not reach those statements.
Use a separate owner-scope drop guard holding a clone of `RuntimeCancellation`,
independent of the lifetime of provider-held signal references.

Suggested regression: a synthetic factory saves the received signal outside its
pending session-creation future. Start selection, wait until that reference is
captured, abort the selector task, and assert the retained signal observes
cancellation. This tests ownership directly and requires no live provider.

**4. Forbidden stream content is rejected only after accumulation.**
The selector progress port observes text and visible/summary reasoning, while
AgentLoop's `consume_stream` can accumulate unsolicited non-text content and
completed tool-call objects before the final outcome is rejected. The empty
registry prevents tool execution, but the selector's incremental byte counter
does not bound those objects. A single unsolicited-tool test proves
non-execution; it does not establish an aggregate output-memory bound. Consider
an existing shared stream guard/observer that rejects forbidden event kinds on
receipt, or explicitly document and test the applicable lower-level aggregate
bounds. This is an output-boundedness concern, not a discovered tool-permission
bypass. A deterministic repeated non-text/tool-event fixture would distinguish
immediate rejection from late accumulation.

### Verification limits and closeout

The four inspected provider-composition tests cover one tool-free captured
request, pre-cancellation and invalid output, unsolicited-tool non-execution and
oversized text, and a provider that never opens until the deadline. Their source
assertions support the intended mechanisms; this pass did not rerun them or use
the coordinator's in-progress full verification as completed evidence. The new
canary, mid-selection host cancellation, owner-drop, and aggregate-event tests
were suggestions, not executed passes at report time.

The implementation retains the ordinary model-ID authority boundary and now
addresses the earlier effect/root/fallback findings. The concrete data-flow and
cancellation findings above remain open until current fixes and targeted evidence
are recorded; their final recheck status follows below. No quality ADOPT claim, live evaluation, original-workspace change,
or installed-library mutation is implied by this review.

## Final bounded recheck: follow-up findings addressed

The coordinator requested a final check after fixing the four provider/host
findings. The evaluator re-read the changed boundary code and regression source,
and inspected the coordinator's existing test receipts. This pass made only
evidence-report edits and performed no live call, production edit, or corpus
change. The statuses below supersede the open status at the time of the previous
review; the original findings remain as the explanation for each repair.

| Finding | Final code-review status | Evidence and remaining scope |
| --- | --- | --- |
| Expanded file contents entering selection | Addressed | `route_with_model_task` prepares from the original task when assistance is active. TUI supplies original `text` to both preparation and PendingSkillRoute, and capability-switch retry retains that original text. The captured-request file-canary regression passes. |
| TUI continuing after selector cancellation | Addressed in all three inspected branches | Direct, VRO, and ReAct return a terminal Cancelled event immediately after resolve when cancellation is set, before injection/advisers/acceptance/compaction. Native-host barrier tests for these branches remain unexecuted. |
| Provider-retained Arc preventing owner-drop cancellation | Addressed | `SelectionOwner` is an independent owner-scope guard holding the local RuntimeCancellation. The retained-signal regression aborts the selector after the provider captures the signal and observes cancellation; it passes. |
| Forbidden event accumulation outside the text cap | Addressed in the shared stream boundary | Selector opts into `with_text_only_response_bound`. Before ordinary accumulation, consume_stream rejects non-text content and tool start/delta/completion events, counts all reasoning/text bytes including opaque reasoning, and caps event count. The unsolicited-tool/oversized-text regression passes; repeated forbidden-event and opaque-reasoning boundary matrices were not separately executed by this evaluator. |

No new concrete defect was found in this bounded recheck. This conclusion is
limited to the inspected paths; it is not an assertion of completed end-to-end
host acceptance or correct model-selected routing.

### Inspected execution receipts

`/tmp/routing-model-provider-tests.log` records:

- `context_paging_composition`: **14 passed**, zero failed, zero ignored,
  completed in 20.01 seconds. The results include
  `expanded_file_contents_only_enter_the_coding_request`,
  `dropping_selector_cancels_a_signal_retained_by_provider`, and the 20-second
  stalled-provider timeout case.
- `skill_routing_settings`: **5 passed**, zero failed, zero ignored.

The file-canary test uses actual reference expansion and captured provider
requests: the selector contains neither expanded file bytes nor the skill body;
the subsequent coding request contains both. It exercises the shared boundary,
not a real terminal/editor process. The retained-signal test waits for the
provider's captured Arc before aborting, so it tests the ownership defect rather
than relying on timing luck.

`/tmp/routing-model-pty.log` reports:
`Skills native PTY: draft/discard/keep/save/restart and library preservation PASS`.
This proves the exercised Settings flow, not native provider-selection
cancellation or adversarial end-to-end routing.

`/tmp/routing-model-msrv-check.log` finishes the coordinator's reported MSRV
all-targets compiler check successfully. The evaluator inspected its completed
output; the coordinator owns the exact command/toolchain receipt. Compilation
does not replace runtime/platform acceptance.

The inspected `/tmp/routing-model-verify.log` was still running its frozen quality
matrix. This review therefore **does not claim full verification completed**.
The coordinator's eventual final receipt must determine that status separately.

SHA-256 of the three completed receipts at inspection:

- Provider/settings: `ef9d73b6d07fff2e290dd08e4615cb67b19e41c65cecf8cee4cc738bd7993f98`.
- Native Skills PTY: `a8c590c1caecad09a2087cccdacb4bf7fb69c7879abfc8e3a817db6f465fa222`.
- Compiler check: `909012eb8d4c9f11086e5c478f78cdfe9204768c91c2b904bacaeeb40abef126`.

### Remaining acceptance limits

- At this review's inspection time, no separately authorized live provider quality
  evaluation had run. The subsequent [completed live evaluation](skill-routing-model-live-evaluation.md)
  supplies that bounded evidence. The original
  independent failure receipt and frozen labels remain unchanged; subsequent
  inspected-corpus runs are regression evidence, not unseen generalization.
- Native TUI/ACP adversarial process tests covering cancellation during selection,
  direct/VRO/ReAct continuation suppression, capability-switch retry disclosure,
  concurrent settings revocation, and persistence canaries are not claimed
  complete by this review.
- The new stream guard was inspected for each forbidden event kind and aggregate
  counter. A dedicated empty-event flood, opaque-reasoning overflow, and repeated
  non-text/tool-event matrix would strengthen executable boundary evidence; the
  inspected existing test only exercises one unsolicited completed tool call and
  oversized text.
- Store-root binding and bounded revision freshness remain the approved snapshot
  scope. Full source/body cryptographic attestation is neither provided nor
  required by this review, and isolated bodies must remain unread during ranking.

The four concrete findings are closed at the code-review level, with the shared
canary, owner-drop, tool/output, and settings regressions supported by inspected
passing receipts. Broader native-host adversarial gates remain unexecuted by this
review. The subsequent live run does not establish fresh-holdout or cross-provider
quality. Shared evidence-index/PRD/DOX integration stays with the
coordinator; no child index or production ownership boundary was changed here.
