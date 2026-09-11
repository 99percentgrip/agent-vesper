# Native completion assurance — research and design proposal

Date: 2026-09-11. Status: approved for implementation; decision in ADR 0028.
Current implementation evidence: [execution report](completion-assurance-execution.md).
Source baseline: `8083f9f131939d14d1f8465f056ffaf071a91cdc`.

## Objective

Prevent a PRD implementation from acquiring a completed status while required
behavior is missing or lacks credible evidence. Make completion a harness decision
derived from a versioned requirement contract and observed results. Models propose
requirements, implementations, tests and findings; they cannot mint verification.

This addresses unsupported completion, including mistaken claims. It does not
establish intent to deceive or guarantee the absence of undiscovered defects.

## Inspected behavior and methods

Read the root, crate, agent, harness, documentation and foundation DOX contracts.
Inspected these source locations with `rg` and `sed`:

- `crates/vesper-agent/src/tools.rs:478–535`: `UpdatePlan` takes model-supplied
  statuses, replaces the task list, writes rendered Markdown and returns it.
- `crates/vesper-agent/src/agent_loop.rs:784–809`: normal stop continues for open
  plan items; otherwise returns `AgentTurnOutcome::Completed` without a
  requirement-to-evidence verdict at this boundary.
- `crates/vesper-agent/src/agent_loop.rs:872–879`: the loop captures the plan tool's
  output. `plan_has_open_items` at 1239 checks Markdown `[ ]` and `[~]` prefixes.
- `crates/vesper-agent/src/agent_loop.rs:1353`: provider content deltas are emitted
  while collecting the stream, before the stop decision.
- `docs/foundation/vro15-repair-execution.md`: component success, native host
  integration, real isolation, scale measurements and exact-commit CI were
  separate acceptance obligations. Earlier milestones did not cover all of them.

Inference: the inspected mechanism enforces continuation while the model reports
open work. It cannot establish PRD satisfaction after the model checks every item,
omits a requirement, replaces the plan, or supplies no plan. `Completed` currently
describes turn termination; it should not double as verified objective completion.
This is source analysis, not a newly executed exploit or a full fresh VRO-15 audit.

## Open-source ideas evaluated

Primary sources inspected on the date above; upstream default-branch URLs can
change. No repositories were cloned, third-party code copied, or tools installed.
Pin source revisions and inspect licenses before any later code reuse.

| Source | Useful idea | Boundary for Vesper |
|---|---|---|
| [BeforeDone](https://github.com/rrrrrredy/beforedone) | Executed-check receipts, file fingerprints and a completion gate | Its README explicitly limits receipts to configured coverage, warns about same-user forgery, and permits an inconclusive warning in one gate case. Vesper must require positive evidence for every required criterion. Do not infer measured reliability gains from the existence of the mechanism. |
| [Spec Kit analysis template](https://github.com/github/spec-kit/blob/main/templates/commands/analyze.md) | Requirement inventory and cross-checks between specification, plan and tasks | Coverage mapping is useful before coding; model analysis alone is not behavioral verification. |
| [OpenSpec verification workflow](https://github.com/Fission-AI/OpenSpec/blob/main/src/core/templates/workflows/verify-change.ts) | Separate completeness, correctness and design coherence | Its workflow uses heuristic source matching and can accept warnings. Required behavior in Vesper needs stronger evidence than likely implementation. |
| [Superpowers verification skill](https://github.com/obra/superpowers/blob/main/skills/verification-before-completion/SKILL.md) | Tie each claim to fresh verification | Instruction discipline complements an enforced boundary; a skill alone cannot own the verdict. |
| [in-toto statement specification](https://github.com/in-toto/attestation/blob/main/spec/v1/statement.md) | Bind typed evidence to immutable artifact digests | Provenance proves the association and origin of a claim, not the adequacy of its tests. |
| [cargo-mutants](https://github.com/sourcefrog/cargo-mutants) | Deliberately change behavior to find tests that fail to detect defects | Useful as a bounded development check for critical invariants, not a mandatory full mutation sweep for every task or a production dependency. |

Recommendation: implement the combined contract/receipt/gate mechanism natively in
Rust. Use these as design references. No reviewed project alone establishes full
PRD coverage or supplies a proven turnkey Vesper integration.

## Workflow

1. **Establish the contract.** Convert the user's PRD and applicable project rules
   into stable requirement IDs. Preserve source spans and the original document
   digest. Each normative section must map to requirements or an explicit,
   justified classification. Unresolved normative text blocks full verification.
   Extract user-visible behavior, failure paths, lifecycle, host/provider/platform
   scope, performance limits, migration and packaging where the PRD requires them.
2. **Challenge the coverage before implementation.** A separate reviewer context
   reads the original PRD and proposed mapping, not merely the implementer's
   summary. It looks for omissions and vague acceptance conditions. Existing clear
   user authorization is sufficient; ask only about material ambiguity or scope
   reductions. Freezing the contract must not add routine confirmation steps.
3. **Implement against observable criteria.** A requirement carries scenarios,
   expected assertions and required evidence classes. Task checkmarks represent
   implementation progress only. They cannot remove requirements or verify them.
4. **Observe verification.** The harness runs admitted checks on an identified
   source snapshot and captures structured results. Tests must actually execute
   the named scenarios. Compilation, empty filters, ignored bodies and assertions
   against mocks cannot satisfy criteria requiring native execution.
5. **Review gaps.** The reviewer inspects the original contract, actual diff,
   production wiring, tests and receipts. It tries counterexamples: unused code,
   missing host registration, cancellation leaks, weak assertions, fake parallelism
   and resource claims unsupported by measurement. Findings require requirement
   IDs and concrete evidence; reviewer confidence is not a receipt.
6. **Decide and continue.** A deterministic evaluator grants verified completion
   only when all required criteria have valid evidence and no blocking findings
   remain. Otherwise it supplies concrete remaining work to the existing bounded
   continuation loop. Cancellation, permission denial, environmental blockers and
   exhausted budgets end with an explicit incomplete outcome, never success.
7. **Render the report.** The harness renders scope, status, evidence and gaps from
   the same decision object used by runtime and release gates. Publishing and local
   installation are separate obligations when requested, not automatic side effects.

The reviewer can be a sequential provider-neutral worker. A second model/provider
may reduce correlated errors but is optional, costs more, and is not a correctness
oracle. Review unavailability cannot be silently converted into a required pass.

## Contract and evidence model

Proposed values, not current APIs:

```rust
enum CriterionState {
    Missing,
    Failed,
    Stale,
    Inconclusive,
    Verified,
}

enum CompletionDecision {
    Verified(CompletionCertificate),
    Continue(Vec<AcceptanceGap>),
    Incomplete { reason: StopReason, gaps: Vec<AcceptanceGap> },
}

trait CompletionGate {
    fn evaluate(&self, input: &AcceptanceSnapshot) -> CompletionDecision;
}
```

`AcceptanceSnapshot` joins a versioned contract, source identity, required check
definitions, execution receipts and review findings. The evaluator is pure and
deterministic. Trait implementations exposed to production cannot accept an
agent-supplied boolean as verification.

A receipt records run/check IDs, contract and verifier-definition digests, source
snapshot digest, relevant toolchain/features/platform/image identity, actual argv
and cwd, terminal process status, scenario/test identities and counts, skipped and
failed cases, bounded artifact references, timestamps and collector identity.
Secrets are excluded from argv; output is bounded and redacted. A command exit
code alone establishes execution status, not scenario coverage.

Start conservatively with a complete declared source-input snapshot, including
untracked required files, build definitions, lockfiles and test inputs. Bound
inventory sizes and refuse unsupported inputs. Use an isolated immutable snapshot
for checks so mutations during a run cannot produce a misleading pass. Recheck
identity at publication. Before/after hashes alone do not exclude a file changing
and then changing back while a verifier reads it.

Reusing unchanged evidence is allowed only for matching contract, source,
configuration, environment and declared freshness policy. Stateful external checks
may have stricter expiry. Uncertain dependency scope invalidates broadly. A newly
edited source tree, PRD, test, workflow or verifier cannot inherit an old success.
Contract revisions preserve lineage; deleting a difficult requirement cannot make
the old objective complete. An authorized reduced scope gets a new, clearly named
verdict rather than retroactively satisfying the original PRD.

## Trust and reporting boundaries

- A model-facing tool may request a check or propose a finding. Only the trusted
  collector writes receipts; only the evaluator issues a completion certificate.
- Keep active policy and acceptance definitions outside worker write authority.
  Workspace edits to them are proposals requiring reconciliation. Agents may
  legitimately improve tests, but weakening required checks invalidates the
  existing contract and triggers review instead of making failure disappear.
- Use the existing permissions and sandbox. Verification does not grant network,
  credentials, deployment, user-state writes or arbitrary shell privileges. Use
  typed argv and the configured execution port, not commands imported from logs.
- Local same-user filesystem signatures are not a hostile-agent security boundary.
  Workers need enforced access restrictions; stronger release assurance needs
  externally controlled CI and protected verification configuration. Hashing or
  signing a self-authored assertion does not establish correctness.
- Durable evidence is opt-in where ACP persistence is opt-in. Keep live state
  outside model context so compaction cannot erase obligations. On a restart with
  no saved trusted evidence, show unverified rather than reconstructing success
  from narrative. Export a bounded evidence bundle only through an explicit flow.
- In a gated implementation run, hold provider prose for the current turn until
  its disposition is known. If it stops without tools, evaluate before publishing
  a final answer. Stream trusted tool/progress events meanwhile. Preserve held
  text in internal history on interruption. Do not rely on a regex detecting
  words such as "done" in arbitrary language.
- The authoritative terminal report is generated from the certificate/gap data.
  Model commentary is not verified status. A final-only hook cannot retract an
  already streamed claim; ACP and TUI must share the publication boundary.
- Reject unknown receipt versions, truncated structured results, missing checks,
  duplicate evidence identities and verifier errors. Required inconclusive or
  skipped outcomes remain incomplete. A check runner being unavailable is a
  blocker with a recovery action, not a pass.
- Gate state and repair attempts have resource/time limits. Existing autonomous
  continuation repairs actionable gaps; repeated no-progress or the ultimate
  ceiling returns an incomplete report. Never force endless retries or bypass
  permissions to earn a green status.

## Rust integration proposal

| Owner | Proposed responsibility |
|---|---|
| `vesper-domain` | Typed requirement, evidence, finding, verdict and host-neutral status values |
| `vesper-agent` | Pure gate policy and completion port; separate turn termination from objective verification; enforce stop and output publication rules |
| `vesper-harness` | Shared contract service, permissioned verifier execution, snapshot/receipt collection, reviewer composition and opt-in persistence |
| TUI and ACP | Same status/report semantics and native controls; host-specific presentation only |
| Direct, ReAct, VRO and Swarm compositions | Carry the same contract identity and gate; worker success never certifies the parent objective |
| `xtask` and CI | Independent contract validation, exact-source verification and release certificate checks |

Prefer modules in existing crates initially; avoid another provider loop or
coordination framework. Preserve MSRV 1.88, safe Rust and current dependency
direction. Any new crate boundary or durable format needs an ADR and DOX update.
No production dependency on the researched Go/Python/TypeScript tools is required.

This protects tasks executed through Vesper only. An external coding agent editing
Vesper requires a separate adapter to the same evaluator plus CI enforcement.
Repository instructions help that agent but cannot give Vesper control over the
external agent's chat stream. A future standalone Rust check command could serve
that adapter; it must report unsupported adapters rather than promise enforcement.

## VRO-15-shaped example

Suppose component tests pass but ACP activation and real parallel workers are absent:

| Requirement | Required evidence | Gate result |
|---|---|---|
| Three independent workers overlap | Native barrier-based overlap with distinct worker/session identities | Missing |
| Both hosts activate the feature | TUI and ACP process tests through persisted controls | ACP missing |
| Cancellation cleans up workers | Actual descendant termination and observed supervisor cleanup | Unit-only evidence insufficient |
| Ledger satisfies specified scale | Defined dimensions/count/latency/memory measurement | Unmeasured scope remains unverified |
| Release is installable | Exact-commit target gates and packaged-payload checks | Pending |

All plan checkmarks and thousands of unrelated passing tests would leave this
objective incomplete. The gate names the missing work; the agent continues within
its authorization. It cannot use a release tag as a substitute for acceptance.

## Required acceptance for this layer itself

1. All-completed, empty, deleted and replaced model plans cannot bypass an active
   unsatisfied contract; missing PRD mappings remain visible.
2. Passing unrelated tests, zero matching tests, ignored real-environment bodies,
   mock-only checks and fabricated stdout cannot satisfy native criteria.
3. Edits after verification, during verification, or to PRD/check definitions
   invalidate or refuse evidence. Wrong platform/features/image/source do too.
4. Receipt forgery from worker tools and attempts to weaken active policy fail
   within the documented isolation boundary; malformed imports fail closed.
5. Fresh valid evidence grants the correct scoped verdict without unnecessary
   reruns. Legitimate user-approved changes preserve old scope and lineage.
6. Reviewer failures and unresolved findings cannot become success; spurious
   findings have an evidence-based resolution path, not an endless repair loop.
7. Both hosts withhold unsupported terminal claims, preserve partial history on
   cancellation, and display incomplete outcomes at the safety ceiling.
8. Direct/ReAct/VRO/Swarm routes and nested workers cannot bypass the parent gate.
   Compaction and opt-in restart/resume preserve or truthfully invalidate state.
9. Default ACP runs create no new durable evidence state. Check execution preserves
   tool permissions, network grants and user-state isolation.
10. Replay representative historical VRO-15 omissions using controlled defective
    fixtures and known-good cases. Measure false completion, false blocking,
    recovered completion, latency, model cost and verification cost. Do not claim
    an improvement rate until this experiment runs; mutation testing is bounded
    to selected critical gate invariants.

## Delivery and current readiness

Implement in reviewable increments: contract/evaluator and adversarial fixtures;
trusted verifier/receipt path; shared stop and publication wiring in both hosts;
coverage review and automatic repair; all execution modes and CI/release integration.
Each increment is partial until the ten acceptance obligations above pass. Do not
advertise the prevention feature after only adding a prompt, reviewer or JSON file.

This document preserves the research and approved delivery contract. Current Rust
implementation, executable gates, local evidence and remaining platform/release
limits are recorded in [the execution report](completion-assurance-execution.md)
and ADR 0028. The baseline diagnosis above is historical source analysis.
External projects supplied design references; no upstream implementation was
copied. Controlled regression results must not be generalized into a live-model
accuracy or zero-defect guarantee.
