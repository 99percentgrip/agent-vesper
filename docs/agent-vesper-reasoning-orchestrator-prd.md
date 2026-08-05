# Product Requirements Document

## Agent Vesper — Vesper Reasoning Orchestrator (VRO)

**Document status:** Draft for research and engineering review  
**Version:** 0.1  
**Date:** 2026-08-01  
**Product:** Agent Vesper  
**Primary integration target:** Agent Vesper runtime and ACP composition layer  
**Initial reference provider/model:** LM Studio with Qwen3.6-27B  
**Scope:** Model-agnostic inference-time reasoning enhancement without modifying model weights

---

## 1. Executive Summary

Agent Vesper shall add a model-agnostic subsystem named the **Vesper Reasoning Orchestrator (VRO)**.

VRO will improve the practical reasoning reliability of supported language models by controlling how inference is performed around the model. It will not retrain, fine-tune, or alter the model’s weights. Instead, it will apply adaptive test-time computation through task decomposition, multiple candidate generation, tool-grounded reasoning, deterministic verification, bounded search, critique, repair, and verified workflow reuse.

The central product insight is that a capable model may fail not because it lacks all required knowledge, but because a single left-to-right response gives it only one opportunity to interpret the task, choose a strategy, execute the strategy, detect errors, and produce a final answer. VRO will separate these responsibilities and selectively spend additional compute only when the task justifies it.

VRO must not become a fixed “think harder” prompt or force every request through an expensive multi-agent loop. It must automatically select the least expensive reasoning workflow likely to meet the task’s reliability requirement.

The initial implementation will target LM Studio running Qwen3.6-27B, while all core contracts must remain provider- and model-independent. The same architecture must later support GLM, OpenAI, Anthropic, Google, local llama.cpp-compatible models, and other Agent Vesper providers.

---

## 2. Problem Statement

A model can demonstrate strong language, coding, and knowledge capabilities while still producing unreliable results on tasks that require:

- Multi-step planning.
- Constraint tracking.
- Repository-level reasoning.
- Tool selection.
- Long-horizon execution.
- Recovery after an incorrect assumption.
- Verification of intermediate decisions.
- Comparison between several plausible approaches.
- Accurate completion under incomplete or conflicting evidence.

A normal single-pass request has several structural weaknesses:

1. **Premature commitment:** the model may choose the first plausible approach and continue even when the premise is wrong.
2. **Error propagation:** an early mistake contaminates later steps.
3. **No independent verification:** the same generation that creates an answer also implicitly accepts it.
4. **Insufficient exploration:** alternative solutions are not compared.
5. **Poor uncertainty handling:** confident language may hide weak evidence.
6. **Weak tool grounding:** the model may reason from memory rather than inspecting the actual environment.
7. **No durable learning from execution failure:** a failed workflow may be repeated in future sessions.
8. **Uniform compute allocation:** trivial and difficult tasks receive essentially the same orchestration pattern.

The product requirement is therefore not to make the underlying model universally “smarter.” The requirement is to make Agent Vesper obtain a more reliable result from the model by providing a superior inference and execution process.

---

## 3. Product Thesis

VRO will improve effective model performance through five mechanisms:

1. **Adaptive decomposition**  
   Convert complex objectives into explicit, bounded subproblems with dependencies and success conditions.

2. **Inference-time search**  
   Generate and compare multiple candidate strategies or answers when one-shot decoding is insufficient.

3. **External grounding**  
   Use tools, repository state, tests, compilers, calculators, retrieval systems, schemas, and other objective signals.

4. **Verification and repair**  
   Validate outputs before commitment and revise only against concrete failures or clearly defined constraints.

5. **Verified workflow memory**  
   Reuse successful reasoning procedures after they have passed objective verification and, where required, user approval.

The orchestrator must prioritize deterministic evidence over model opinion. A compiler failure is stronger evidence than a model critic saying that code “looks correct.” A schema validator is stronger than a model’s confidence in its JSON. A repository search is stronger than an assumption about the codebase.

---

## 4. Goals

### 4.1 Primary Goals

VRO shall:

- Improve task success rates for supported local and remote models without changing model weights.
- Work with models of different sizes, reasoning styles, context lengths, and tool-calling capabilities.
- Automatically select an appropriate reasoning workflow.
- Spend additional tokens, latency, and tool calls only when justified.
- Prefer deterministic and environment-grounded verification.
- Support coding, technical investigation, planning, factual research, structured generation, and general reasoning.
- Integrate with Agent Vesper’s existing cancellation, bounded-channel, session, event-ordering, and provider abstractions.
- Produce stable, inspectable orchestration events without exposing raw private chain-of-thought.
- Learn reusable workflows only after verification.
- Preserve a direct low-latency path for simple tasks.

### 4.2 Secondary Goals

- Permit optional heterogeneous collaboration, such as one model proposing and another verifying.
- Enable offline evaluation of reasoning strategies against a fixed benchmark corpus.
- Expose clear user controls such as Auto, Fast, Balanced, Deep, and Maximum.
- Support local-only operation with LM Studio and local tools.
- Provide a foundation for future Vesper IDE reasoning visualization and agent status animation.
- Allow provider-specific optimizations without leaking provider assumptions into the core runtime.

---

## 5. Non-Goals

The first release shall not:

- Retrain or fine-tune model weights.
- Claim to transform a weak model into a frontier model in every domain.
- Guarantee correctness where no reliable evidence or verifier exists.
- Persist or display unrestricted raw chain-of-thought.
- Allow unbounded recursive self-reflection.
- Execute arbitrary tools without the existing permission and sandbox model.
- Use multi-agent debate for every request.
- Replace the model provider abstraction with LM Studio-specific code.
- Automatically save a workflow merely because the model says it succeeded.
- Optimize benchmark scores at the expense of real Agent Vesper workflows.
- hide additional compute usage from the user or administrator.

---

## 6. Key Product Principles

### 6.1 Orchestration, Not Prompt Decoration

VRO must be a runtime subsystem with state, policies, budgets, verification contracts, and events. Prompt templates are implementation details, not the product architecture.

### 6.2 Evidence Before Confidence

Confidence expressed by a model is not proof. VRO must rank evidence approximately as follows:

1. Deterministic verifier or executable oracle.
2. Direct environment observation.
3. Independent retrieval or source evidence.
4. Agreement between genuinely diverse candidates.
5. Independent model verifier.
6. Same-model self-critique.
7. Unverified model confidence.

### 6.3 Cheapest Successful Strategy

The policy engine must select the lowest-cost workflow expected to satisfy the task. A greeting should remain a single call. A repository migration may require planning, parallel investigation, execution, tests, and repair.

### 6.4 Bounded Deliberation

Every workflow must have explicit limits:

- Maximum model calls.
- Maximum generated tokens.
- Maximum wall-clock duration.
- Maximum tool calls.
- Maximum search depth.
- Maximum candidate count.
- Maximum repair attempts.
- Cancellation checkpoints.

### 6.5 Verifier Diversity

The same model should not be the only generator, critic, judge, and source of truth when stronger external checks exist.

### 6.6 Provider-Agnostic Core

Core orchestration must operate through capability contracts. Provider adapters may expose special features, but no strategy may assume that every model supports native reasoning controls, tool calls, structured output, log probabilities, or persistent response state.

### 6.7 No Raw Chain-of-Thought Dependency

VRO must function through structured artifacts such as plans, assumptions, evidence, candidate outputs, constraint checks, and concise rationale summaries. Provider-private reasoning content must not be required for correctness, rendered to users by default, or stored as durable memory.

---

## 7. Target Users and Use Cases

### 7.1 Primary User

A developer using Agent Vesper as a coding and technical agent, including local inference through LM Studio.

### 7.2 Core Use Cases

#### A. Repository Investigation

User asks Agent Vesper to identify why a feature is broken.

VRO should:

1. Profile the task as repository-grounded and medium/high complexity.
2. Require source inspection before proposing a fix.
3. Generate an investigation plan.
4. Use repository tools.
5. Track hypotheses and evidence.
6. Reject hypotheses contradicted by the code.
7. Produce a fix only after identifying a supported root cause.
8. Run relevant tests or static checks.
9. Repair failures within budget.

#### B. Coding Implementation

User requests a feature.

VRO should:

1. Extract requirements and constraints.
2. Inspect architecture and conventions.
3. Produce a dependency-aware plan.
4. Implement in bounded steps.
5. Verify compilation, tests, formatting, and architecture rules.
6. Repair concrete failures.
7. Report completed work, evidence, and unresolved risks.

#### C. Technical Planning

User requests an implementation plan.

VRO should:

1. Distinguish known facts from assumptions.
2. investigate the actual repository when available.
3. Produce multiple architecture candidates for high-impact decisions.
4. Compare trade-offs.
5. Validate the selected plan against constraints and existing components.
6. Return a plan with acceptance criteria and rollback boundaries.

#### D. Mathematical or Logical Reasoning

VRO should:

1. Generate one or more solution paths when complexity warrants it.
2. Extract a structured final answer.
3. Verify using a calculator, symbolic engine, executable script, or consistency check.
4. Revise only when a check fails.

#### E. Factual Research

VRO should:

1. retrieve sources.
2. separate claims from evidence.
3. check source agreement and freshness.
4. identify unresolved conflicts.
5. prevent unsupported factual assertions from being finalized.

#### F. Structured Output

VRO should:

1. request a schema-constrained response when supported.
2. validate the output.
3. repair only the invalid fields.
4. fail explicitly when the schema cannot be satisfied within budget.

#### G. Workflow Learning

After a task succeeds:

1. VRO extracts the reusable procedure.
2. It removes task-specific secrets and transient values.
3. It attaches preconditions and verification requirements.
4. It replays the workflow in a controlled validation case.
5. It saves only after objective success and required approval.

---

## 8. User Experience

### 8.1 Reasoning Modes

Agent Vesper shall expose the following user-facing modes:

| Mode | Behavior |
|---|---|
| **Auto** | VRO profiles the request and selects the workflow and budget. Default. |
| **Fast** | Single pass or minimal plan-and-check. Strict latency and token ceiling. |
| **Balanced** | Decomposition plus one verification/repair cycle when needed. |
| **Deep** | Multiple candidates, stronger verification, and bounded search. |
| **Maximum** | Highest configured test-time budget, intended for difficult or high-value tasks. |
| **Off** | Bypass VRO and use the provider’s normal direct response path. |

Auto mode is the product’s primary behavior. Manual modes are overrides, diagnostics, and user preference controls.

### 8.2 Status Surface

The TUI and ACP event stream may show phase-level states such as:

- Understanding request.
- Inspecting context.
- Building plan.
- Exploring alternatives.
- Running tools.
- Validating result.
- Repairing failed checks.
- Finalizing answer.

The UI must not stream raw hidden reasoning. It may show concise, user-safe summaries such as “Comparing two implementation strategies” or “Tests failed in the session serializer; attempting one repair.”

### 8.3 Result Summary

For non-trivial tasks, the final result should optionally include:

- Strategy used.
- Verification performed.
- Evidence of success.
- Remaining uncertainty.
- Budget consumed.
- Whether a reusable workflow was learned or proposed.

This information must be concise by default and expandable in diagnostic views.

---

## 9. System Architecture

```text
User / ACP Client
        |
        v
Agent Vesper Request Router
        |
        v
+------------------------------------------+
| Vesper Reasoning Orchestrator            |
|                                          |
|  1. Task Profiler                        |
|  2. Capability Registry                  |
|  3. Policy and Budget Engine             |
|  4. Workflow Planner                     |
|  5. Candidate/Search Executor            |
|  6. Tool and Environment Bridge          |
|  7. Verifier Registry                    |
|  8. Repair Controller                    |
|  9. Workflow Memory                      |
| 10. Evaluation and Telemetry             |
+------------------------------------------+
        |
        +------> Provider Adapter(s)
        |          - LM Studio
        |          - GLM
        |          - OpenAI
        |          - Anthropic
        |          - Google
        |
        +------> Tools / MCP / Repository / Tests
        |
        +------> Session Persistence and Replay
        |
        v
Validated Final Response + ACP Events
```

---

## 10. Core Components

### 10.1 Task Profiler

The profiler converts a user request and available context into a structured `TaskProfile`.

Required fields:

- Task domain.
- Estimated complexity.
- Consequence/risk level.
- Need for external evidence.
- Need for repository inspection.
- Tool availability.
- Verifier availability.
- Ambiguity level.
- Expected output type.
- Estimated context requirement.
- Parallelization opportunity.
- Whether the task is reversible.
- Whether user approval is required before execution.

The profiler must be cheap. It may use deterministic heuristics first and a small structured model call only when necessary.

Example output:

```json
{
  "domain": "coding",
  "complexity": "high",
  "risk": "medium",
  "requires_environment_grounding": true,
  "requires_plan": true,
  "candidate_count_hint": 2,
  "available_verifiers": ["cargo_check", "cargo_test", "clippy"],
  "recommended_strategy": "plan_execute_verify_repair"
}
```

### 10.2 Model and Provider Capability Registry

Each provider/model combination must declare observed capabilities rather than relying only on model family names.

Capability fields should include:

- Native tool-call support.
- Default/emulated tool-call support.
- Structured output support.
- Reasoning-control support.
- Streaming support.
- Cancellation behavior.
- Maximum context.
- Recommended output budget.
- Parallel request capacity.
- Stateful response support.
- Vision support.
- Reliable stop sequences.
- Quantization and runtime metadata.
- Known prompt-template requirements.
- Empirical tool-call success rate.
- Empirical schema-conformance rate.
- Empirical long-context degradation profile.

Capabilities must be probeable. Agent Vesper should be able to run a small provider certification suite and store the results.

### 10.3 Policy Engine

The policy engine chooses a `ReasoningStrategy` from the task profile, capability registry, configured mode, historical evaluation data, and available budget.

Initial strategies:

1. `direct`
2. `plan_then_answer`
3. `plan_execute_verify`
4. `generate_verify_repair`
5. `parallel_candidates_consensus`
6. `parallel_candidates_judge`
7. `tool_grounded_react`
8. `bounded_tree_search`
9. `proposer_critic_adjudicator`
10. `workflow_replay_with_verification`

The policy must be deterministic for identical inputs when configured in deterministic mode.

### 10.4 Budget Manager

A `ReasoningBudget` must include:

```rust
pub struct ReasoningBudget {
    pub max_model_calls: u32,
    pub max_total_output_tokens: u64,
    pub max_tool_calls: u32,
    pub max_parallel_branches: u16,
    pub max_search_depth: u16,
    pub max_repairs: u16,
    pub max_wall_time_ms: u64,
}
```

The budget manager must:

- Reserve capacity for finalization.
- Stop low-value branches.
- account for provider rate limits.
- Respect cancellation immediately.
- Prevent retries from silently exceeding user limits.
- Emit budget-exhaustion reasons.
- Permit strategy downgrade when the preferred workflow cannot fit.

### 10.5 Workflow Planner

The planner produces a structured, executable reasoning graph rather than prose alone.

Each step must define:

- Step identifier.
- Objective.
- Dependencies.
- Allowed tools.
- Expected output schema.
- Verification method.
- Failure policy.
- Maximum attempts.
- Whether steps may run in parallel.
- Whether user approval is required.

Example:

```json
{
  "steps": [
    {
      "id": "inspect_runtime",
      "objective": "Locate request dispatch and cancellation boundaries",
      "depends_on": [],
      "tools": ["repo_search", "read_file"],
      "verify_with": ["evidence_present"]
    },
    {
      "id": "design_change",
      "objective": "Produce a minimal architecture-compatible patch plan",
      "depends_on": ["inspect_runtime"],
      "tools": [],
      "verify_with": ["constraint_check"]
    },
    {
      "id": "implement",
      "objective": "Apply the approved design",
      "depends_on": ["design_change"],
      "tools": ["patch"],
      "verify_with": ["cargo_check", "cargo_test"]
    }
  ]
}
```

### 10.6 Candidate and Search Executor

The executor supports:

- Diverse sampling.
- Candidate-specific prompts.
- Same-model parallel candidates.
- Cross-model candidates.
- Beam-style pruning.
- Limited tree expansion.
- Candidate deduplication.
- Early stopping when a verifier establishes success.
- Branch cancellation.
- Preservation of evidence and tool results.

Candidate diversity must not be simulated merely by asking for “three alternatives” in one completion. For high-value tasks, candidates should use separate inference calls with controlled variation in strategy, assumptions, or decoding.

### 10.7 Tool and Environment Bridge

VRO must integrate with Agent Vesper’s existing tool system rather than invent a separate execution layer.

The bridge must provide:

- Tool capability descriptions.
- Permission checks.
- Sandboxing.
- Timeouts.
- Structured results.
- Side-effect classification.
- Idempotency metadata.
- Redaction of secrets.
- Replay-safe result recording where possible.
- Clear distinction between observation and mutation tools.

Read-only evidence collection should be preferred before mutation.

### 10.8 Verifier Registry

Verifiers are the central reliability mechanism.

Verifier types:

#### Deterministic Verifiers

- JSON schema validation.
- Compiler.
- Unit/integration tests.
- Linter.
- Formatter check.
- Type checker.
- Arithmetic calculator.
- Symbolic solver.
- Regex or parser.
- File existence and content checks.
- Database constraints.
- Protocol transcript matcher.
- Hash or fixture integrity check.
- Security policy checks.

#### Evidence Verifiers

- Required repository files inspected.
- Claims linked to source evidence.
- Dates and versions checked.
- Contradictory sources identified.
- Requirement-to-output traceability.

#### Model-Based Verifiers

- Constraint critic.
- Factual consistency critic.
- Plan completeness critic.
- Independent candidate judge.
- Adjudicator.

Model-based verification is fallback or supplementary evidence. It must never override a failed deterministic verifier without an explicit policy exception.

A verifier response should contain:

```rust
pub struct VerificationResult {
    pub verifier_id: String,
    pub status: VerificationStatus,
    pub confidence: f32,
    pub findings: Vec<VerificationFinding>,
    pub evidence_refs: Vec<EvidenceRef>,
    pub repairable: bool,
}
```

### 10.9 Repair Controller

The repair controller receives explicit verifier findings and creates a bounded correction request.

Requirements:

- Repair the smallest failed scope.
- Preserve validated work.
- Include exact failure evidence.
- Avoid repeating an identical failed attempt.
- Stop when no new evidence or strategy exists.
- Escalate to a different strategy when appropriate.
- Never enter an unrestricted “review again” loop.

### 10.10 Workflow Memory

Workflow memory stores reusable procedures, not raw conversations.

A saved workflow must contain:

- Name and version.
- Applicable task signature.
- Preconditions.
- Required tools.
- Ordered or graph-based steps.
- Parameter schema.
- Verification requirements.
- Known failure modes.
- Provenance.
- Validation history.
- Approval state.
- Expiration or revalidation policy.

Workflow states:

1. `candidate`
2. `validated`
3. `approved`
4. `active`
5. `quarantined`
6. `deprecated`

A workflow becomes active only after passing the configured validation gate.

### 10.11 Evaluation and Telemetry

VRO must record enough information to determine whether additional reasoning actually helped.

Metrics:

- Baseline direct success.
- Orchestrated success.
- First-pass success.
- Success after repair.
- Tool-call accuracy.
- Schema-conformance rate.
- Verifier false-positive and false-negative rates.
- Candidate agreement.
- Model calls per task.
- Tokens per successful task.
- Wall-clock latency.
- Cancellation responsiveness.
- Budget exhaustion.
- Strategy selection accuracy.
- User override rate.
- Workflow replay success.
- Regression rate on simple tasks.

Telemetry must support local-only storage and configurable redaction.

---

## 11. Reasoning Strategies

### 11.1 Direct

Use for simple, low-risk tasks.

Flow:

```text
request -> model -> response
```

No extra orchestration beyond normal validation.

### 11.2 Plan Then Answer

Use when decomposition helps but external execution is unnecessary.

```text
request -> structured plan -> constraint check -> final answer
```

### 11.3 Generate, Verify, Repair

Use when a strong verifier exists.

```text
request -> candidate -> verifier
                    -> pass -> final
                    -> fail -> targeted repair -> verifier
```

This should be the default enhanced strategy for code, structured outputs, and calculations with available checks.

### 11.4 Parallel Candidates with Consensus

Use when the answer is expected to converge and no perfect verifier exists.

```text
request -> candidate A
        -> candidate B -> normalize -> compare -> consensus/fallback
        -> candidate C
```

Consensus must compare normalized final answers and supporting evidence, not just wording similarity.

### 11.5 Parallel Candidates with Judge

Use when several plausible designs require trade-off analysis.

The judge must receive candidates in randomized order to reduce position bias. Where possible, the judge should be a different model or a separately prompted verifier.

### 11.6 Tool-Grounded ReAct

Use when progress depends on observing an environment.

```text
state -> decide next action -> tool -> observation -> update state -> ...
```

The loop must operate on structured action and observation records. Maximum steps and side effects are enforced by the budget and permission layers.

### 11.7 Bounded Tree Search

Use only for tasks that benefit from exploring strategic branches.

Examples:

- Difficult debugging with several competing root-cause hypotheses.
- Architecture choices with irreversible consequences.
- Constraint-heavy planning.
- Multi-step puzzles.
- Complex migration sequencing.

Initial release limits should be conservative, such as depth 3 and branching factor 2, with aggressive pruning.

### 11.8 Proposer, Critic, Adjudicator

Use when objective verification is weak but failure cost justifies additional model calls.

Roles may be served by:

- The same model with isolated contexts.
- Different local models.
- A local proposer and remote verifier.
- A stronger model only for adjudication.

The adjudicator must evaluate explicit criteria, not select the most persuasive prose.

### 11.9 Reflection After External Failure

Reflection is allowed only after a concrete signal such as:

- Test failure.
- Tool error.
- User correction.
- Contradictory evidence.
- Invalid schema.
- Permission denial.
- Benchmark failure.

The reflection artifact should contain:

- What assumption failed.
- What evidence exposed the failure.
- What change would prevent repetition.
- Whether the lesson is task-specific or reusable.

---

## 12. Automatic Strategy Selection

The initial policy can be rule-based, then later learned from evaluation data.

Example policy:

```text
IF task is simple AND risk is low
    -> direct

ELSE IF deterministic verifier exists AND no environment interaction is needed
    -> generate_verify_repair

ELSE IF environment evidence is required
    -> tool_grounded_react + verifier

ELSE IF multiple plausible answers exist AND cost is acceptable
    -> parallel_candidates_judge

ELSE IF task has long-horizon dependencies
    -> plan_execute_verify

ELSE
    -> plan_then_answer
```

Factors increasing reasoning depth:

- High ambiguity.
- High consequence.
- Multiple explicit constraints.
- Large repository scope.
- Prior failed attempt.
- Low observed model reliability for the task class.
- Availability of strong verifiers.
- User-selected Deep or Maximum mode.

Factors reducing reasoning depth:

- Simple conversational request.
- Strict latency budget.
- No useful verifier or tool.
- Low remaining context.
- Provider saturation.
- Repeated candidate convergence.
- Early deterministic success.

---

## 13. LM Studio and Qwen3.6-27B Initial Integration

### 13.1 Integration Requirements

The LM Studio adapter must support:

- Model discovery.
- Server health check.
- OpenAI-compatible chat or responses endpoint.
- Streaming.
- Cancellation.
- Tool calls where supported.
- Structured JSON output where supported.
- Provider-specific extra parameters.
- Context and output token configuration.
- Capability probe results.
- Local-network and localhost deployment.
- Model load/unload awareness when available.

### 13.2 Qwen3.6-27B Profile

The initial profile must not assume that the full-precision model card behavior exactly matches every GGUF quantization or LM Studio runtime configuration.

The certification suite must empirically measure:

- Reasoning-mode behavior.
- Tool-call format compliance.
- JSON schema compliance.
- Context retention.
- Long-output stability.
- Quantization-specific regressions.
- Temperature sensitivity.
- Candidate diversity.
- Self-critique reliability.
- Coding and repository task performance.
- Stop and cancellation behavior.

### 13.3 Provider-Specific Optimization Boundary

Qwen-specific parameters and chat-template options belong in the LM Studio/Qwen capability adapter. They must not appear in the generic VRO policy or workflow schema.

---

## 14. Data Contracts

### 14.1 Reasoning Request

```rust
pub struct ReasoningRequest {
    pub request_id: RequestId,
    pub session_id: SessionId,
    pub user_message: String,
    pub context_refs: Vec<ContextRef>,
    pub mode: ReasoningMode,
    pub risk_hint: Option<RiskLevel>,
    pub budget_override: Option<ReasoningBudget>,
    pub privacy_mode: PrivacyMode,
}
```

### 14.2 Task Profile

```rust
pub struct TaskProfile {
    pub domain: TaskDomain,
    pub complexity: Complexity,
    pub risk: RiskLevel,
    pub ambiguity: f32,
    pub requires_grounding: bool,
    pub requires_mutation: bool,
    pub available_verifiers: Vec<VerifierId>,
    pub recommended_strategy: ReasoningStrategy,
}
```

### 14.3 Deliberation Artifact

```rust
pub struct DeliberationArtifact {
    pub objective: String,
    pub constraints: Vec<String>,
    pub assumptions: Vec<Assumption>,
    pub plan: Vec<PlanStep>,
    pub evidence: Vec<EvidenceRef>,
    pub unresolved_questions: Vec<String>,
}
```

This structure is a concise operational record. It is not raw chain-of-thought.

### 14.4 Candidate

```rust
pub struct Candidate {
    pub candidate_id: CandidateId,
    pub strategy_variant: String,
    pub output: StructuredOutput,
    pub evidence: Vec<EvidenceRef>,
    pub verification: Vec<VerificationResult>,
    pub cost: InferenceCost,
}
```

### 14.5 Final Outcome

```rust
pub struct ReasoningOutcome {
    pub status: OutcomeStatus,
    pub final_output: Option<StructuredOutput>,
    pub selected_candidate: Option<CandidateId>,
    pub verification_summary: VerificationSummary,
    pub unresolved_risks: Vec<String>,
    pub cost: InferenceCost,
}
```

---

## 15. State Machine

```text
RECEIVED
   |
   v
PROFILED
   |
   v
STRATEGY_SELECTED
   |
   v
PLANNED (optional)
   |
   v
EXECUTING
   |
   +----> TOOL_WAIT
   |         |
   |         v
   |      EXECUTING
   |
   v
VERIFYING
   |
   +---- pass ----------------> FINALIZING
   |
   +---- repairable failure --> REPAIRING --> VERIFYING
   |
   +---- terminal failure ----> FAILED
   |
   v
FINALIZING
   |
   v
COMPLETED
```

Every active state must accept cancellation and transition to `CANCELLED` with preserved partial output and evidence according to existing Agent Vesper semantics.

---

## 16. ACP Event Requirements

New internal events may include:

- `reasoning.profiled`
- `reasoning.strategy_selected`
- `reasoning.plan_created`
- `reasoning.branch_started`
- `reasoning.branch_pruned`
- `reasoning.tool_requested`
- `reasoning.tool_completed`
- `reasoning.verification_started`
- `reasoning.verification_failed`
- `reasoning.repair_started`
- `reasoning.candidate_selected`
- `reasoning.budget_warning`
- `reasoning.completed`

Public ACP compatibility must be preserved. Where ACP has no dedicated event, VRO events should be translated into existing session update or status mechanisms without changing required top-level wire fields.

Event ordering must remain deterministic within a session. Parallel branch events must carry branch identifiers and monotonic sequence numbers.

---

## 17. Safety, Privacy, and Security

VRO increases the number of model and tool interactions, so the attack surface also increases.

Requirements:

- Preserve existing permission gates.
- Treat retrieved content and tool output as untrusted.
- Prevent prompt injection from overriding system policy or tool permissions.
- Redact secrets before sending content to remote verifier models.
- Permit local-only reasoning mode.
- Classify tools as read-only, reversible mutation, or irreversible mutation.
- Require approval for configured high-impact operations.
- Limit recursive calls and tool loops.
- Record provenance for external evidence.
- Do not store provider-private reasoning tokens by default.
- Do not promote a workflow containing credentials, personal data, absolute machine-specific paths, or unreviewed destructive actions.
- Quarantine workflows after repeated verification failure.

---

## 18. Failure Modes and Required Handling

| Failure | Required behavior |
|---|---|
| Model cannot follow structured schema | Retry once with reduced schema, then fall back or fail explicitly. |
| Tool call is malformed | Normalize only when unambiguous; otherwise return a structured tool error to the model. |
| Candidates disagree | Invoke verifier/adjudicator if budget permits; otherwise surface uncertainty. |
| Same repair repeats | Stop that branch and change strategy or fail. |
| Verifier is unavailable | Downgrade confidence and avoid claiming verified success. |
| Budget is exhausted | Return best validated partial result with a clear limitation. |
| Provider disconnects | Preserve session state and retry only under configured idempotency rules. |
| Cancellation occurs | Stop new work, cancel active calls, preserve allowed partial output, emit terminal cancellation event. |
| Context limit is approached | Compress evidence and operational state; never discard active constraints silently. |
| Tool output conflicts with model assumption | Tool evidence wins unless the tool itself is invalidated. |
| Model judge selects unsupported answer | Reject selection if mandatory evidence or deterministic checks fail. |
| Workflow replay fails | Quarantine or downgrade the workflow and fall back to normal planning. |

---

## 19. Evaluation Plan

### 19.1 Evaluation Principle

VRO must be compared against the same model, same provider, and same task set using the provider’s direct baseline. The product succeeds only if orchestration improves verified task completion enough to justify its cost.

### 19.2 Benchmark Layers

#### Layer A: Deterministic Unit Scenarios

- JSON schema generation.
- Arithmetic.
- Constraint satisfaction.
- Tool selection.
- Malformed tool-call repair.
- Cancellation.
- Budget exhaustion.
- Branch ordering.
- Replay determinism.

These run in normal CI with mocked or fixture-backed providers where appropriate.

#### Layer B: Local Model Certification

Run against the actual LM Studio process and selected Qwen3.6-27B quantization.

Measure:

- Direct versus orchestrated accuracy.
- Tool-call success.
- Schema compliance.
- Latency.
- Tokens.
- Cancellation.
- Long-context behavior.
- Repeatability.

#### Layer C: Domain Benchmarks

Recommended categories:

- Coding: HumanEval/MBPP subsets and Agent Vesper repository tasks.
- Math/reasoning: GSM8K and selected MATH-style problems.
- Tool use: fixed function-calling scenarios.
- Planning: constraint-heavy implementation and migration cases.
- Repository reasoning: internally curated issue-to-fix tasks.
- Factual verification: source-grounded questions with known evidence.

#### Layer D: Real Project Regression Corpus

Build a frozen corpus from actual Agent Vesper development tasks, including:

- ACP protocol changes.
- Session persistence.
- Cancellation and backpressure.
- CI debugging.
- Repository architecture reviews.
- Multi-file implementation.
- Documentation grounded in source state.

### 19.3 Required Metrics

Primary:

- Verified task success rate.
- Regression rate against direct mode.
- Cost per verified success.
- Median and p95 latency.
- Repair success rate.
- Deterministic verifier pass rate.

Secondary:

- Candidate agreement.
- Strategy selection quality.
- User interruption rate.
- User override rate.
- Workflow reuse success.
- Unsupported-confidence rate.

### 19.4 Initial Acceptance Targets

These are engineering gates, not guaranteed universal gains:

1. At least **10 percentage points** absolute improvement over direct baseline on the selected difficult local-model evaluation suite.
2. No more than **2 percentage points** regression on simple tasks.
3. At least **95%** valid orchestration-state transitions.
4. At least **99%** deterministic event-ordering success in concurrency tests.
5. Cancellation acknowledged within the existing runtime target.
6. No unbounded loops in fuzz and soak testing.
7. At least **90%** successful structured-output conformance on models certified as schema-capable.
8. At least **20%** lower failure recurrence after a concrete verifier-guided repair compared with blind regeneration.
9. Auto mode average cost no greater than **3× direct mode** across the mixed evaluation suite.
10. Fast mode p95 latency no greater than **1.5× direct mode**.

Targets may be revised after the research phase establishes realistic baselines.

---

## 20. Research Phase Requirements

Implementation must not begin with a large architecture commitment before a focused research and prototype phase.

### R0. Baseline Characterization

- Select the exact Qwen3.6-27B LM Studio quantization and runtime settings.
- Record direct performance on the frozen benchmark corpus.
- Measure tokens, latency, context use, tool calling, and schema output.
- Produce a failure taxonomy.

### R1. Strategy Experiments

Evaluate at minimum:

- Plan then answer.
- Generate/verify/repair.
- Same-model self-critique.
- Tool-grounded critique.
- Self-consistency with 3 and 5 candidates.
- Independent judge.
- Limited tree search.
- Reflection after concrete failure.

Each strategy must be tested under identical tasks and budgets where possible.

### R2. Verifier Reliability Study

For each task class:

- Identify deterministic verifiers.
- Measure false positives and false negatives.
- Compare same-model critic, independent model critic, and deterministic checks.
- Determine which verifier combinations justify repair.

### R3. Budget Curves

Measure quality against:

- Model call count.
- Candidate count.
- Output-token budget.
- Search depth.
- Repair count.
- Wall-clock latency.

The outcome must identify the point of diminishing returns.

### R4. Capability Probe Prototype

Build a provider/model probe that measures:

- Tool-call support.
- Structured output.
- Streaming.
- Cancellation.
- context limit behavior.
- Reasoning controls.
- Parallel capacity.

### R5. Architecture Decision Record

The research phase must end with:

- Experimental results.
- Recommended minimum viable strategy set.
- Rejected approaches and reasons.
- Data contracts.
- integration boundaries.
- Revised acceptance targets.
- Implementation sequence.
- Explicit go/no-go recommendation.

---

## 21. Implementation Plan

To avoid conflict with Agent Vesper’s existing numbered stages, this feature uses the prefix `VRO`.

### VRO-1 — Core Contracts and Direct Compatibility

Deliver:

- Core data types.
- Capability registry.
- Task profile schema.
- Budget manager.
- State machine.
- Direct strategy.
- Feature flag.
- No behavior regression when disabled.

Exit criteria:

- Existing tests remain green.
- Direct ACP transcripts remain compatible.
- Cancellation and bounded channels remain intact.

### VRO-2 — Structured Plan and Deterministic Verification

Deliver:

- Plan schema.
- Generate/verify/repair strategy.
- Verifier registry.
- Initial schema, compiler, test, and constraint verifiers.
- Targeted repair loop.
- Verification events.

Exit criteria:

- Fixed deterministic scenarios pass.
- Repair loops are bounded.
- Verified results cannot be finalized after mandatory verifier failure.

### VRO-3 — LM Studio Provider Certification

Deliver:

- LM Studio health/model discovery.
- Capability probe.
- Qwen3.6-27B profile.
- Structured output adapter.
- Tool-call normalization.
- Real-process integration tests.

Exit criteria:

- Reproducible certification report.
- Real-process cancellation and streaming tests.
- Known limitations documented by model/quantization.

### VRO-4 — Parallel Candidates and Consensus

Deliver:

- Candidate executor.
- Parallel branch isolation.
- Candidate normalization.
- Consensus and judge strategies.
- Branch pruning and budget accounting.

Exit criteria:

- Deterministic branch/event ordering.
- No cross-branch state leakage.
- Demonstrated gain on selected reasoning suite.

### VRO-5 — Tool-Grounded Reasoning

Deliver:

- Structured action/observation loop.
- Evidence references.
- Read-before-write policy.
- Tool error repair.
- Prompt-injection defenses.
- Side-effect-aware permissions.

Exit criteria:

- Tool scenarios pass through the real process path.
- Mutation tools cannot bypass approvals.
- Evidence-grounded answers outperform memory-only baseline.

### VRO-6 — Bounded Search and Adjudication

Deliver:

- Limited tree/beam search.
- Proposer/critic/adjudicator workflow.
- Strategy-specific pruning.
- Diminishing-return stop rules.

Exit criteria:

- Search cannot exceed configured depth or budget.
- Search is activated only for eligible task classes.
- Measurable gain justifies compute on the target subset.

### VRO-7 — Verified Workflow Learning

Deliver:

- Workflow extraction.
- Generalization and secret stripping.
- Validation replay.
- Approval state.
- Versioning.
- Quarantine and deprecation.

Exit criteria:

- No workflow becomes active without passing its gate.
- Failed workflows are quarantined automatically.
- Replay is deterministic where the environment permits.

### VRO-8 — UX, Diagnostics, and Release Hardening

Deliver:

- Auto/Fast/Balanced/Deep/Maximum/Off controls.
- ACP phase events.
- TUI status integration.
- Per-task diagnostic report.
- Benchmark dashboards or reports for developers, not end-user KPI walls.
- Soak, fuzz, concurrency, and failure-injection testing.

Exit criteria:

- Acceptance metrics met.
- No protocol regression.
- Resource use documented.
- Rollback feature flag tested.

---

## 22. Testing Requirements

### 22.1 Unit Tests

- Task-profile serialization.
- Policy selection.
- Budget consumption.
- State transitions.
- Candidate deduplication.
- Verifier precedence.
- Repair-loop termination.
- Workflow validation states.
- Redaction.
- Provider capability fallback.

### 22.2 Integration Tests

- Real LM Studio process.
- Streaming and cancellation.
- Parallel candidates.
- Tool invocation.
- malformed output.
- Provider disconnect.
- Session resume.
- Context compression.
- Workflow replay.
- ACP event translation.

### 22.3 Property and Fuzz Tests

- No negative budget.
- No transition from terminal states.
- No final success after mandatory verifier failure.
- No branch count above configured maximum.
- No mutation without permission.
- No infinite repair/search loop.
- Stable serialization and deserialization.
- Arbitrary malformed provider payload handling.

### 22.4 Soak Tests

- Long sessions.
- Repeated Deep-mode requests.
- Slow consumer.
- Backpressure.
- Provider stalls.
- Cancellation storms.
- Large tool output.
- Memory growth.
- Parallel sessions with same-session serialization.

---

## 23. Observability

Each reasoning run should have a trace containing:

- Request and session identifiers.
- Model/provider profile.
- Selected strategy and reason.
- Budget allocation and consumption.
- Step and branch timings.
- Tool calls and result classes.
- Verifier outcomes.
- Repair attempts.
- Final status.
- Error category.
- Redacted evidence references.

Logs must permit reconstruction of orchestration decisions without requiring storage of raw hidden reasoning.

---

## 24. Configuration

Example configuration:

```toml
[reasoning]
enabled = true
default_mode = "auto"
persist_private_reasoning = false
allow_cross_provider_verification = false
max_global_parallel_branches = 4

[reasoning.fast]
max_model_calls = 1
max_repairs = 0
max_wall_time_ms = 30000

[reasoning.balanced]
max_model_calls = 4
max_repairs = 1
max_parallel_branches = 2

[reasoning.deep]
max_model_calls = 10
max_repairs = 2
max_parallel_branches = 3
max_search_depth = 3

[reasoning.workflow_memory]
enabled = true
require_approval = true
revalidate_after_days = 30
```

Configuration precedence:

1. Safety and administrator hard limits.
2. Provider hard limits.
3. User request override.
4. Session preference.
5. Global default.
6. Strategy recommendation.

---

## 25. Rollout Strategy

### Stage A: Developer-Only Feature Flag

- VRO disabled by default.
- Run shadow evaluation alongside direct mode without affecting user output.
- Compare direct and orchestrated candidates offline.

### Stage B: Opt-In LM Studio Preview

- Enable Auto and explicit modes.
- Limit strategies to direct, plan, verify/repair, and tool-grounded reasoning.
- Collect local diagnostics.

### Stage C: Default Auto for Certified Models

- Enable only for model/provider profiles meeting minimum reliability.
- Preserve one-click Off mode.
- Add automatic downgrade under load.

### Stage D: Multi-Provider General Availability

- Certify additional providers.
- Add cross-model verification.
- Enable verified workflow learning.

Rollback must be possible through a single feature flag that restores the existing direct runtime path.

---

## 26. Open Questions

1. Which Qwen3.6-27B quantization provides the best quality/latency balance on the target RTX 4080 Super system?
2. Does retaining provider reasoning context improve multi-turn coding enough to justify context cost?
3. Is same-model candidate diversity sufficient, or is a second local model needed for meaningful independence?
4. Which task-profile decisions can be deterministic rather than model-generated?
5. What verifier confidence threshold should trigger repair?
6. How should VRO combine contradictory deterministic and model-based signals?
7. Can KV-cache or prompt-prefix reuse materially reduce parallel candidate cost in the selected LM Studio runtime?
8. Which workflows are safe to learn automatically, and which always require approval?
9. How should VRO compress evidence in long sessions without losing active constraints?
10. Should Deep mode allow remote verification when the primary model is local?
11. What is the minimum useful tree-search configuration before overhead exceeds gains?
12. How should provider-private reasoning tokens be handled when an API returns them separately?
13. Can strategy selection later be trained from VRO telemetry without creating opaque or unstable behavior?

---

## 27. Risks

### 27.1 Cost and Latency Explosion

Mitigation:

- Auto routing.
- Hard budgets.
- Early stopping.
- verifier-driven branch pruning.
- direct mode for simple tasks.
- empirical budget curves.

### 27.2 Same-Model Blind Spots

Mitigation:

- Deterministic tools.
- Independent evidence.
- optional heterogeneous verifier.
- isolated contexts.
- explicit uncertainty.

### 27.3 Self-Critique Degradation

A model may change a correct answer into an incorrect one when asked to “review itself” without new evidence.

Mitigation:

- Trigger repair from concrete findings.
- Preserve validated output.
- Require verifier evidence.
- Limit blind self-reflection.

### 27.4 Orchestration Complexity

Mitigation:

- Start with a small strategy set.
- Strict state machine.
- typed contracts.
- feature flags.
- extensive transcript tests.

### 27.5 Benchmark Overfitting

Mitigation:

- Include real Agent Vesper tasks.
- Freeze evaluation sets.
- maintain held-out tasks.
- measure cost and regressions, not only accuracy.

### 27.6 Learned Workflow Contamination

Mitigation:

- Validation replay.
- approval gates.
- versioning.
- secret stripping.
- quarantine.
- expiration and revalidation.

### 27.7 Model Capability Variance

Mitigation:

- Empirical capability probes.
- per-model profiles.
- provider-specific adapters.
- graceful strategy downgrade.

---

## 28. Research Foundation

The design is informed by established inference-time reasoning approaches:

- **Self-Consistency Improves Chain of Thought Reasoning in Language Models** — multiple sampled solution paths can improve answer selection compared with a single greedy path.
- **ReAct: Synergizing Reasoning and Acting in Language Models** — interleaving reasoning with external actions grounds the model in environment observations.
- **Tree of Thoughts: Deliberate Problem Solving with Large Language Models** — explicit search over alternative reasoning states can help tasks requiring exploration and backtracking.
- **CRITIC: Large Language Models Can Self-Correct with Tool-Interactive Critiquing** — external tools can provide concrete feedback for correction.
- **Reflexion: Language Agents with Verbal Reinforcement Learning** — feedback from completed attempts can be converted into reusable episodic guidance without weight updates.
- **Language Agent Tree Search** — bounded search can combine reasoning, acting, planning, and external feedback.
- **Large Language Models Cannot Self-Correct Reasoning Yet** — unsupported intrinsic self-correction can fail or degrade performance, reinforcing the requirement for external evidence and strict verification.

These methods are inputs to the research program, not features that should be copied blindly. VRO must validate each strategy on Agent Vesper’s actual models, runtimes, and workloads.

---

## 29. Definition of Done

VRO is complete for initial release when:

1. Agent Vesper can run direct and orchestrated requests through the same provider abstraction.
2. Auto mode selects among at least direct, plan/verify/repair, parallel candidate, and tool-grounded strategies.
3. LM Studio with the selected Qwen3.6-27B configuration passes the provider certification suite.
4. Mandatory deterministic verifier failures block false success.
5. Repair and search are strictly bounded.
6. Cancellation, backpressure, event ordering, and session semantics remain correct.
7. The target evaluation suite shows a material verified-success improvement over direct mode within the approved cost ceiling.
8. Simple-task regressions remain within the accepted threshold.
9. Raw hidden reasoning is neither required nor persisted by default.
10. The subsystem can be disabled without changing existing Agent Vesper behavior.
11. Verified workflows can be proposed, validated, versioned, approved, replayed, and quarantined.
12. The engineering report documents performance, limitations, provider-specific behavior, and rejected strategies.

---

## 30. Final Product Position

VRO should be presented as a **reasoning reliability and inference orchestration system**, not as a promise that any small model becomes equivalent to the largest available model.

Its value is concrete:

- Better use of the capability already present in a model.
- More deliberate strategy selection.
- More grounding in real tools and project state.
- Fewer unverified conclusions.
- Recovery from concrete failures.
- Reuse of proven workflows.
- Controlled allocation of additional inference compute.

For a model such as Qwen3.6-27B running locally through LM Studio, VRO can provide the model with a stronger problem-solving process than a normal one-shot chat request. The research phase must determine exactly how much improvement is achievable, on which task classes, and at what token and latency cost.
