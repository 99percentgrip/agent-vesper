# Routing request recognition and independent evaluation

Date: 2026-09-13. Status: IN PROGRESS; quality adoption remains HOLD.
PRD: [Skill routing quality](../skill-routing-quality-prd.md).

## Objective

Improve natural task recognition without teaching the router benchmark answers,
changing the skill library, or weakening eligibility and completion gates.
The previous [language follow-up](skill-routing-language-execution.md) reached
90% positive Recall@3. This follow-up is not a completion claim.

## Methods and files

`crates/vesper-memory/src/routing_quality.rs` currently experiments with a bounded
WordNet 3.0 verb lexicon for recognizing imperative requests. Conversation controls
remain excluded. A generic request verb does not by itself establish a specialist
topic: the document must have its own identity match or other topic evidence.
The full query remains available to contract rejection checks.

`crates/vesper-memory/assets/routing-verbs.txt` contains 8,429 sorted single-word
ASCII verbs and the complete upstream license. The unchanged upstream archive is
[WordNet 3.0](https://wordnetcode.princeton.edu/3.0/WordNet-3.0.tar.gz), SHA-256
`640db279c949a88f61f851dd54ebbb22d003f8b90b85267042ef85a3781d3a52`.
The asset SHA-256 is
`33555a6dd1dcfc8b6df24047460b3a531a1b9bf711f7ac63a2089e0d38db987d`.
Reproduction reads only `COPYING` and `dict/index.verb` from the supplied archive;
it does not unpack files or download anything:

```sh
python3 docs/foundation/skill-routing-language-assets.py /path/to/WordNet-3.0.tar.gz /tmp/routing-verbs.txt
cmp crates/vesper-memory/assets/routing-verbs.txt /tmp/routing-verbs.txt
```

The reproduced asset matched byte-for-byte. New regression tests cover sourced
verbs, stop/pause/wait/hold controls, generic verbs without topic evidence, and
an unrelated catalog identity restoring relevance to the wrong document.

## Exact evidence so far

The [initial verb experiment](skill-routing-verbs-initial-results.json) reached
36/40 positive Recall@3 at 100/250/500 entries, but activated skills on 2/20
no-skill prompts. This is a regression, retained rather than promoted.

The [first predicate-separation experiment](skill-routing-predicate-results.json)
kept 36/40 positive Recall@3 at 100/250/500 entries and reduced incorrect no-skill
activation to 1/20; the unnamed subset remained 27/31 with one abstention.
At 25 entries Recall@3 was 37/40 with zero negative activation. All required size
gates still fail. The complete unchanged-corpus test finished in 195.35 seconds.
Its results precede the additional per-document identity fix, so they do not
attest that later source change.

At that checkpoint the focused request-recognition suite passed 11/11 and its
development diagnostic passed. Current verification is recorded in the
[model-assisted implementation report](skill-routing-model-assistance-execution.md). The earlier full verification log `/tmp/routing-verbs-verify.log`
ends with 23/23 acceptance cases, but predates predicate separation; it is not
current-source verification.

## Independent evaluation and unresolved work

Alex authorized a separate evaluator to author and freeze fresh cases from the
actual bounded catalog before seeing implementation or predictions. The frozen 205-case corpus and independent review are complete; see the
[independent record](skill-routing-independent-execution.md). The original corpus has already been examined and is regression
evidence, not an unseen holdout. No threshold or label has been changed here.

Final adoption requires current-source evidence, independent labels, the PRD's
positive/no-skill/sibling/no-name gates and the remaining native/integration checks.
The GLM-owned score-floor PR-2/PR-3 work is outside this checkout's ownership.
No release, version bump, installation or skill-library mutation is authorized
by this experiment. Linux offline tests do not establish live-model effectiveness.

## DOX and readiness effect

The memory asset child contract records provenance and derived-data ownership.
Its parent indexes that boundary. Foundation documentation tracks experiments and
failed measurements. Standard remains the default; this work has no promotion or
release readiness effect until its remaining checks pass.

## Subsequent diagnostics and implementation

General negation, informational-clause and complementary-topic handling was added
without changing frozen labels. A failed global request-form gate was reverted;
its [receipt](skill-routing-independent-request-gate-results.json) remains available.
Development reached 38/40 positives with 0/20 no-skill activation; these inspected
examples are not held-out acceptance. The performance fixture now expects one
selection for 500 duplicate-topic entries, because redundant topic matches are no
longer treated as complementary work. Its catalog size/timing loop is unchanged;
separate composition fixtures cover genuinely distinct requested tasks.

Alex approved the [model-assisted amendment](../skill-routing-model-assistance-proposal.md).
The [native implementation report](skill-routing-model-assistance-execution.md)
tracks that next step and its separately gated live quality work. These changes do
not convert previous lexical failures into a promotion pass.

Current lexical-only independent regression: [complete receipt](skill-routing-independent-current-results.json).
Enhanced records 82/95 positive Recall@3, 5 abstentions, 17/60 no-skill
activations and 7/30 forbidden-sibling activations. These fail promotion.

The [current four-size lexical matrix](skill-routing-current-quality-results.json)
retains every prediction and three ordering permutations. At 500 entries its
held-out-labelled positive Recall@3 is **27/40**, with one false abstention;
no-skill activation is 0/20 and sibling recall is 18/20 with zero harmful
sibling selections. This is a positive-recall regression versus earlier receipts,
not a passing tradeoff. These previously inspected labels remain regression data.
