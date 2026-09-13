# Skill routing quality: video review and PRD execution

Date: 2026-09-13. Status: **planning deliverable complete; implementation unstarted**.
Owner PRD: [Skill routing quality](../skill-routing-quality-prd.md).

## Objective

Review Alex's linked video, compare the proposed direction with current Vesper,
and produce a bounded, evidence-grounded PRD rather than implement an unmeasured
new router.

## Source access and methods

- YouTube browser fetch returned cache misses. Network-sandbox curl failed DNS;
  approved public network access retrieved YouTube oEmbed and watch-page metadata.
- The author is Cloud Codes and title is “Every Skill You Add Breaks Your Harness
  (Here's the Fix)”: https://www.youtube.com/watch?v=YnXl7bb3V0o .
- Parsed `ytInitialPlayerResponse.videoDetails.shortDescription` for the creator's
  argument, chapter headings and original source links. Public English ASR caption
  metadata existed, but the caption request returned zero bytes. No full video
  viewing or transcript review is claimed; secondary transcript search results
  were discovery aids, not technical evidence.
- Creator chapters place expanding libraries at 0:59, selection versus context
  at 1:48, missed invocation at 2:55, disclosure at 4:13, truncation at 6:23,
  and pruning/search/content routing at 6:57. These are description timestamps,
  not independently verified transcript quotations.
- Read primary paper abstracts and official tool-routing documentation linked in
  PRD §2. Paper versions inspected: shadowing v2, SkillRouter v5, same-family
  retrieval v2. No paper code or benchmark was executed. Model-specific empirical
  findings motivate experiments and are not advertised as Vesper results.
- Commands: `curl` for public metadata/captions, Python stdlib JSON extraction,
  `rg`/`sed` for local code, `git status --short`, `git rev-parse HEAD`, Markdown
  link/anchor validation and `git diff --check`. No provider requests or installs.

Temporary inspection files: `/tmp/vesper-routing-video.html`,
`/tmp/vesper-video-captions.json`, `/tmp/vesper-video-caption.json`. Signed caption
URLs are not durable evidence. The public source links, version references and
access limitations above make the finding understandable without those files.

## Local findings

Inspected HEAD `5cf5835f78d8e779443188c3490172d7736a589a` and current working tree.
The earlier dollar-token repair was already uncommitted and is preserved.

- `skill_orchestrator.rs::{SkillRoutingQuery, score_candidate, rank_chunks,
  parse_metadata}`: existing deterministic metadata-only routing, three-skill
  cap, bounded chunk loading, policy enforcement and original score-positive
  chunk gate. The separate chunk-floor PRD remains pending in the inspected tree.
- `skills.rs::{headline_of, read_prefix}`: 120-character display/fallback headline
  is distinct from frontmatter routing description; 32,000-byte catalog prefix
  and 500-entry cap. The video does not establish that Vesper shares another
  harness's particular truncation bug.
- ADR 0024 prohibits body text in ranking. Therefore runtime body-aware retrieval
  is a deferred contract change, not a silent extension of metadata routing.
- Both hosts already share orchestration. New relevance behavior should extend
  that shared path rather than introduce adapter-specific routers.

## Files and proposed direction

- `docs/skill-routing-quality-prd.md`: ten traceable requirements; richer bounded
  descriptors, local candidate retrieval, sibling execution-contract checks,
  explicit abstention, diagnostics/Settings, optional embedding experiment,
  240-query labeled evaluation with distractor scaling, adoption/HOLD gates and
  five implementation units.
- `docs/AGENTS.md`: new PRD ownership; `docs/README.md`: specification link.
- `docs/foundation/evidence-index.md`: planning receipt; this execution report
  links back to the PRD and records unresolved research limits.

## Verification, deviations and readiness

Changed Markdown local paths and anchors resolve; `git diff --check` passes.
No runtime tests rerun because this task changes only planning documentation.
Earlier repair test receipts are not evidence that this proposal works.

Full video/transcript content remains unavailable in this session. The PRD is
based on verified creator metadata, inspected primary sources and local code,
with that boundary stated prominently. If a transcript later adds material
constraints, amend the proposal rather than pretending they were reviewed.
No implementation, training, live-agent effectiveness trial, benchmark run,
commit, push, release or local installation performed for this planning task.
Proposed latency/quality thresholds require baseline measurement and are not
performance claims. Full-body routing would require a separately accepted ADR
and root/local contract change. Existing code and pending fixes remain intact.

## DOX closeout

Root, documentation and foundation chains read. Documentation ownership updated
for the new PRD; existing foundation ownership covers this report. Root and
foundation AGENTS remain unchanged because global rules and child boundaries do
not change. No child index entries added or removed. The PRD, documentation
landing page and evidence index link the planning artifacts.

## User clarification: preserve the library

Alex explicitly requires keeping the full skill library. PRD §1 and R11 now
require unchanged source inventories and hashes across indexing, routing and
rollback; selection is temporary and no automatic removal, archive, disable or
source rewrite is allowed. Existing-library descriptor edits require separate
authorization. The documentation ownership contract records this preference.
Documentation checks: R11 and its preservation acceptance are present;
`git diff --check` passes. Runtime preservation tests are required future evidence,
not claimed executed by this documentation update.

## User clarification: intelligent task matching

The primary objective is automatically identifying the correct skill for a natural
user task, not merely limiting loaded skills. PRD §1 now states this explicitly;
the evaluation requires at least 60 positive paraphrases without skill names or
exact trigger phrases, with separate recall and missed-selection acceptance.
Documentation ownership records this preference. These remain proposed tests;
no claim of achieved semantic accuracy is made. `git diff --check` passes.
