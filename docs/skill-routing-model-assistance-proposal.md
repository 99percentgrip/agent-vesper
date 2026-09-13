# Bounded model-assisted skill selection

Status: approved by Alex on 2026-09-13; native implementation and separately authorized live regression complete. Quality promotion remains HOLD; see the execution report.
Owner: [Skill routing quality PRD](skill-routing-quality-prd.md).
Evidence: [independent evaluation](foundation/skill-routing-independent-execution.md).

## Problem and proposed behavior

The first independent native measurement found an acceptable skill on 84/95
positive tasks, but activated on 16/60 no-skill prompts and exposed forbidden
siblings on 20/30 cases. Pure lexical ranking confuses topic mention with an
instruction and cannot reliably distinguish several similar procedures.
Subsequent diagnostic changes improve some cases but do not pass adoption gates.
A pretrained embedding-only experiment also failed abstention requirements.

Propose a native opt-in that asks the user's already configured provider/model to
interpret the task and choose from a bounded eligible metadata shortlist before
any automatically selected body is loaded. It may explicitly choose no skill.
No separate account, local model installation, library rewrite or model training.
This is a proposed interpretation step, not a guarantee of correct routing.

## Concrete implementation boundary

1. Keep Standard and Enhanced lexical modes operational. Add a separate native
   opt-in using the existing grouped Settings save flow and equivalent ACP control.
2. Prepare at most 12 policy-eligible candidates from the existing metadata index.
   Carry stable identities, source revisions and bounded purpose/action/artifact
   metadata. No skill body, isolated payload, file contents or project scan enters
   the selection request. Refuse oversized metadata rather than silently truncate.
3. Use the existing provider-neutral native agent/worker boundary with no tools,
   one iteration, bounded output and a 20-second cancellation deadline.
   At most one selection request per new task, plus the existing single refinement
   allowance only when new task information arrives. Never replay a tool call.
4. The input contains the current user task and at most 8 KiB of candidate metadata;
   oversized task input falls back observably to lexical routing. Request a strict
   structured result: selected IDs (at most three), no skill needed, or ambiguity.
   Reject out-of-shortlist IDs, duplicates, malformed output and excessive output.
5. Before loading, recheck live source revisions, settings, platform, tools,
   invocation policy, effects/resources, conflicts and existing body/chunk budgets.
   A model decision never counts as explicit user invocation, permission, resource
   availability, or verification evidence. Isolated bodies remain worker-only.
6. Compose the final bodies transiently using the existing cross-host direct,
   VRO and ReAct injection paths. Keep the original user message/history intact.
   Provider/parse/cancellation failures report a bounded fallback, never fabricated
   successful model-assisted selection. Keep network work off the TUI render loop.
7. Trace IDs, outcome and bounded reason codes; do not persist selection prompts,
   bodies, hidden reasoning or credentials. Expose added selection latency/usage
   rather than claiming the existing local-index latency covers a provider call.

## Approved authorization and acceptance boundary

The approved PRD D6 says: “No automatic model installation or remote upload of
skill content.” This proposal adds transmission of bounded skill metadata to the
already configured provider, plus an extra provider request with potential latency
and account usage. That data flow and cost are outside the current lexical plan;
Alex explicitly approved this native opt-in data flow and added call on 2026-09-13. Alex subsequently approved one live evaluation of at most 205 tool-free GLM-5.3 selector calls, each bounded to 20 seconds and 1,024 requested output tokens.

The local index retains its 50 ms warm p95 budget. Model interpretation must have
separately reported latency and usage, with a 20-second upper deadline;
it cannot be represented as meeting the local-only timing gate.

No live provider call is authorized by this proposal. Foundation verification
continues to use offline fixtures for plumbing, cancellation, strict parsing,
authority boundaries and TUI/ACP parity. Such fixtures do not prove model routing
quality. Adoption still requires scope-appropriate quality evidence and a fresh
independent assessment; previously inspected corpora remain regression evidence.
The separately authorized evaluation measures the configured provider's decisions;
its receipts belong in the implementation report and do not authorize additional passes. GLM-owned score-floor changes, release work and
Alex's installed payload remain outside this proposal's mutation scope.

Implementation and verification: [execution report](foundation/skill-routing-model-assistance-execution.md).

Snapshots bind store roots, bounded catalog metadata, size/mtime freshness, settings and task identity. They are not cryptographic attestations of unread body suffixes or chunk bytes; current loaders retain their normal bounds and validation. Index failure and stale model decisions withhold activation. Input overflow and provider/parse failure use an explicitly reported lexical fallback; user cancellation stops selection.
