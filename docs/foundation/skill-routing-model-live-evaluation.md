# Model-assisted routing: independently scored live regression

Status: **205-case live regression completed; measured improvement with retained
failures and unsupported compatibility cases. No full adoption claim.**

## Objective and scope

Independently score the single user-authorized native live evaluation against the
[unchanged frozen corpus](skill-routing-independent-corpus.json), distinguishing
the model's decision from retrieval, fallback behavior, native final selection,
and unsupported execution contexts. The implementation agent launched the run;
this evaluator performs only offline reading/scoring and makes no provider calls.

The corpus has already informed implementation and review. This is explicitly an
**inspected-corpus regression**, not a fresh holdout or an independent estimate of
unseen production traffic. Its 205 cases comprise 95 positives, 60 no-skill,
30 sibling, 12 resource/permission, and 8 ambiguous cases.

## Inputs and methods

- Frozen corpus SHA-256:
  `79ef89a4934a0e2c61aa08d4e2de6a3efd0272c18f1ee3e9507271d5c6111275`.
- [Native JSONL receipt](skill-routing-model-live-receipt.jsonl), captured initially
  at `/tmp/routing-model-live-20260913.jsonl`. Its header records
  provider `zai`, model `glm-5.3`, coding plan, enabled reasoning, 205 cases, and
  488 source-library files. Header values describe this run, not a cross-provider
  claim.
- Launcher source: `apps/agent-vesper-tui/examples/skill_routing_eval.rs`. It calls
  the shared native selector once per prepared case, writes each outcome, and
  checks the skill-library manifest after finishing. This evaluator did not
  restart, retry, modify, or invoke that launcher.
- [Scoring helper](skill-routing-model-live-summary.py) uses only Python's standard
  library. It validates the frozen digest and receipt IDs, retains every case,
  and emits detailed per-case scores plus cohort statistics. It never edits
  labels, receipts, implementation, or skill files.
- Current implementation/source identity belongs to
  [the coordinator's verification record](skill-routing-model-verification.json).
  It is separate from quality scoring and does not turn build/test success into
  a passing routing result.

Reproduce the complete scoring result:

```sh
python3 docs/foundation/skill-routing-model-live-summary.py docs/foundation/skill-routing-model-live-receipt.jsonl --baseline docs/foundation/skill-routing-independent-current-results.json --output docs/foundation/skill-routing-model-live-results.json
```

For temporary output, replace the output path with `/tmp/routing-model-live-summary.json`.
During execution only, `--allow-partial` returns explicitly provisional results.
The default rejects missing cases, duplicate IDs, mismatched hashes, and incomplete
trailing records. Provisional mode ignores only an unfinished final line, never
a malformed completed record. Complete 205-row coverage alone does not prove
successful process exit or the final library-invariance check; inspect the
launcher's completion log separately.

## Scoring and attribution

- The frozen expected IDs are acceptable alternatives, not a requirement to
  select every listed skill. Clear positive/sibling cases require a nonempty
  subset, without unexpected or forbidden additions, capped at three unique IDs.
- No-skill/resource labels require empty output. Only explicitly ambiguous cases
  permit optional abstention. No labels are changed after observing results.
- Both the raw valid model decision and the final native selection are scored.
  Conflicts, eligibility checks, or fallback can make these differ. Reporting a
  final success does not automatically credit the model.
- `validated_model_decision` means successful selector output with a nonempty
  offered list. Successful empty-shortlist abstention is a deterministic no-call
  path. Errors with lexical fallback and absence of a prepared selector are
  separate categories.
- A provider-path attempt is not necessarily an HTTP/model call: authentication,
  context, or session creation can fail before dispatch. The receipt cannot
  establish an exact call count for every error, so the scorer does not invent one.
- Retrieval availability reports whether an expected ID was offered and its
  shortlist rank. A model cannot recover an ID missing from the offered set.
  The live receipt does not include expected-target rejection reasons, so absence
  combines native eligibility and retrieval limits; it is not all blamed on the
  model or relabeled as correct abstention.
- All 12 resource cases retain explicit unsupported status and receive no
  supported-success credit. Their prompts can still be inspected for semantic
  abstention, but that does not prove native permission/resource enforcement.
- Latency uses recorded selector milliseconds and nearest-rank p50/p95/p99,
  separated by decision/fallback status and empty versus nonempty shortlist.
  It does not measure main-task latency or complete evaluation wall time.
- Usage sums only available cumulative provider snapshots, by field and recorded
  provenance. Reasoning is not added to output, nor cached input to input, because
  those fields can overlap. Unknown usage and cost stay unknown; no monetary
  charge or quota is inferred.
- Main recall, false-activation, and forbidden-sibling rates include 95% Wilson
  intervals. These are descriptive binomial-model intervals on a curated,
  nonrandom, potentially correlated corpus, not confidence bounds for production
  traffic. Baseline deltas compare identical frozen IDs against current Standard;
  forbidden-sibling exposure is compared with forbidden exposure on both sides,
  never with the broader count of unexpected additions. The optional baseline
  also identifies current Enhanced eligibility rejections without removing any
  case from the authoritative full-cohort result.

## Scorer verification

Offline synthetic checks exercised correct positive selection, unwanted extra
activation, positive abstention failure, required no-skill abstention, forbidden
sibling activation, and optional ambiguous abstention. A complete synthetic
205-row empty-output receipt yielded exactly 60 supported no-skill successes,
zero positive successes, and 12 semantically empty resource results with **zero**
supported resource passes. Partial coverage, an unfinished final line, and a
duplicate receipt ID were checked; incomplete runs and duplicates were rejected
as intended. Temporary synthetic receipt files were removed and never mixed with
the live run.

Additional synthetic checks verified Wilson boundary cases (0/60 and 60/60),
matched-case Standard comparison, and preservation of the positive cohort's
95 authoritative cases alongside the 89-case eligibility diagnostic.

The helper successfully read an in-progress snapshot, explicitly marked it
provisional, and retained generic provider errors as lexical fallbacks. Final
scoring began only after all 205 rows existed and the completion log was available.

## Completion and immutable result identity

The coordinator reported process exit 0. The evaluator independently read the
completion log's exact line:

```text
LIVE regression finished: 205 cases, 205 prepared, 488 source files unchanged
```

The JSONL contains one header plus 205 unique case records, with no missing or
unfinished rows. The scorer completed successfully. The original frozen labels
and initial independent failed-native receipt remain unchanged.

- Raw live receipt SHA-256:
  `9558a2f69bee358f673e0286acbd39c5ce3fd62c506f310be32db12de73b8ccf`.
- [Detailed scored results](skill-routing-model-live-results.json) SHA-256:
  `a8c82ff0ba2f51fbb64d4ce0fbc752503db2a6f2f3209efedf4f47ea1297d4dc`.
- [Current native baseline](skill-routing-independent-current-results.json)
  SHA-256: `38b45416b954db9fe00650ae4b979abc8f74301de5a7466b6101180002d1d4b8`.

## Full-cohort results and current Standard comparison

The following are final native selections, including any lexical fallbacks.
All original denominators remain authoritative. Lower is better for false
activation/exposure; higher is better for recall/strict success. Percentage-point
deltas compare identical cases and the same metric on both sides.

| Metric | Live result; 95% Wilson interval | Current Standard; 95% Wilson interval | Delta |
| --- | --- | --- | ---: |
| Positive recall | 89/95 = 93.7%; 86.9–97.1% | 60/95 = 63.2%; 53.1–72.2% | +30.5 pp |
| Positive strict success | 84/95 = 88.4%; 80.4–93.4% | 39/95 = 41.1%; 31.7–51.1% | +47.4 pp |
| No-skill false activation | 0/60 = 0%; 0–6.0% | 5/60 = 8.3%; 3.6–18.1% | −8.3 pp |
| Forbidden sibling exposure | 2/30 = 6.7%; 1.8–21.3% | 10/30 = 33.3%; 19.2–51.2% | −26.7 pp |

Sibling strict success was **26/30**; two remaining cases abstained, and two
selected a forbidden alternative. All **8/8 ambiguous** cases satisfied their
unchanged labels (five acceptable selections and three abstentions).

All **12/12 resource/permission cases remain unsupported**. Ten returned empty
selections and two selected the specifically prohibited skill. These semantic
outcomes provide **zero supported compatibility passes**. They are not removed
from coverage or counted as evidence of permission enforcement.

The intervals are descriptive binomial-model intervals on this curated set.
Zero observed no-skill activations does not establish a zero production false
activation rate; the interval and sampling limitations remain material.

## Model decisions, deterministic outcomes, and fallbacks

| Path | Cases | Attribution |
| --- | ---: | --- |
| Validated model decision with nonempty shortlist | 180 | Actual completed model selection |
| Provider error followed by lexical fallback | 5 | No valid model decision; final selection belongs to fallback |
| Empty-shortlist deterministic abstention | 20 | No provider selection call needed |

All 20 deterministic no-call cases belong to the no-skill cohort. The other 40
no-skill cases received valid model decisions and all abstained. The 180 valid
model decisions and their native final selected IDs agreed exactly in this run.

Five cases returned the same bounded error, `Model selection provider failed`:
`independent-009`, `-014`, `-057`, `-077`, and `-178`. The receipt does not retain
the underlying error category; this report does not infer a timeout, token-limit,
rate-limit, or authentication cause. The launcher performed no retry/restart.
Fallback selected the expected skill in `-009`, `-014`, and `-077`; those three
strict successes are **not** credited to the model. `-057` selected unrelated
skills and `-178` selected the forbidden note-store sibling.

The positive cohort has 91 valid model decisions and four fallbacks. Of its 84
strict final successes, **81 are model decisions and three are fallback results**.
Among the 86 valid-model positive cases whose expected skill was offered, the
model hit the expected ID in 86 and passed strict selection in 81.

## Positive failures and retrieval ceiling

| Scope | Positive recall | Positive strict success | Meaning |
| --- | ---: | ---: | --- |
| Authoritative complete positive cohort | 89/95 | 84/95 | Required labels unchanged; 11 strict failures remain |
| Diagnostic: no current native target rejection | 89/89 | 84/89 | Does not replace the 95-case acceptance denominator |

Expected skills appeared in **89/95 positive shortlists**. The six absent targets
exactly match the current native Enhanced rejection receipt:

- `independent-050` (`imessage`), `-064` (`findmy`), `-090`
  (`apple-reminders`), and `-091` (`apple-notes`): unsupported Linux platform.
- `independent-052` (`huggingface-hub`, a download request) and `-057`
  (`github-repo-management`, a fork request): external-side-effect request gate.

For those six missing-offer cases, the model abstained in `-050`, `-052`, and
`-090`; substituted `computer-use` in `-064` and `-091`; and the failed provider
path in `-057` yielded unrelated lexical fallback. The model could not select an
absent ID, but substituting another operation remains wrong under the frozen
task labels. There were no additional positive shortlist misses beyond these
six recorded eligibility exclusions.

Five strict failures had the correct skill offered and selected, but added an
unrequested sibling:

| Case | Expected selection | Additional selected skill |
| --- | --- | --- |
| `independent-007` | `vesper-skill-authoring` | `harness-skill-gate-mechanics` |
| `independent-012` | `teams-meeting-pipeline` | `meeting-action-items` |
| `independent-019` | `simplify-code` | `merge-reconciler` |
| `independent-031` | `popular-web-designs` | `claude-design` |
| `independent-068` | `email-inbox-triage` | `himalaya` |

These remain label failures; no post-result widening of acceptable IDs was
performed. The diagnostic 89/89 recall identifies a useful ceiling distinction,
not permission to drop platform/gate failures or claim all routing gates pass.

## Sibling and unsupported-context failures

Expected IDs appeared in **26/30 sibling shortlists**, and every one of those
26 cases passed strict final selection. The four missing targets were the same
eligibility families: `-168` repository setup and `-174` Hub download, plus `-177`
Reminders and `-178` Apple Notes. `-168` and `-177` abstained; `-174` selected
forbidden `llama-cpp`; `-178` fell back to forbidden `obsidian` after a provider
failure. The live and Standard exposure comparison above uses forbidden IDs on
both sides, not the broader extra-activation metric.

The unsupported resource cases `independent-191` and `-193` selected `claude-code`
and `airtable` despite their explicit unavailable/denied execution context. The
other ten abstained. Since native mappings for these contexts were not supported
by this evaluation, neither the failures nor the empty results are represented
as validated native permission enforcement. The full unsupported set remains
listed in the detailed results.

## Observed latency and provider usage

The nonempty-shortlist selector path ran on 185 cases, including its five error
outcomes. Its median was **3.184 seconds**, p95 **10.010 seconds**, p99 **12.731
seconds**, and maximum **13.254 seconds**. Mean time was 4.215 seconds. The 180
valid-model decisions alone had median 3.150 seconds and p95 9.347 seconds.
The five error/fallback paths averaged 11.720 seconds. The 20 deterministic
empty-shortlist paths recorded 0 milliseconds and are not included in the
nonempty-path quantiles.

Summed recorded selector elapsed time was **779.685 seconds**. This is not total
evaluation wall time or the local-index latency metric. No selector receipt
reported a timeout string, and all observed elapsed values were below the
configured 20-second deadline; a single run does not prove a universal latency
guarantee.

180 receipts contain cumulative usage, all with `exact` provenance for these
fields:

| Provider-reported field | Sum |
| --- | ---: |
| Input | 136,011 |
| Output | 41,704 |
| Total | 177,715 |
| Reasoning, overlapping output | 39,339 |
| Cached input, overlapping input | 31,552 |

Usage is unavailable for the five error paths; the 20 deterministic no-call paths
also carry no usage object. **177,715 is the recorded total for available
receipts, not a complete billing total.** Reasoning/cached fields are not added
again. No price, charge, remaining allowance, or quota is inferred.

## Deviations, unresolved items, and readiness

The native launcher does not emit exact dispatch counters, target-rejection
taxonomy, end-to-end wall-clock duration, or a final manifest hash in each JSONL
row. The scorer preserves these evidence limits rather than filling them with
inference. The coordinator owns process completion, current source/library
invariance, and the final verification record.

The completed run improves recall, no-skill abstention, and forbidden-sibling
exposure against current Standard on the same inspected cases. It does not close
all routing acceptance: 11 positive strict failures, two forbidden sibling
exposures, five provider errors, and 12 unsupported compatibility cases remain
visible. Native-host adversarial process coverage, additional provider/model
behavior, and fresh independent generalization remain separate gates.

This evaluator owns the scoring helper, this report, and its derived results;
the coordinator owns the copied raw receipt and shared PRD/evidence-index/DOX
integration. No production, library, or frozen-label change was made during this
scoring work.
