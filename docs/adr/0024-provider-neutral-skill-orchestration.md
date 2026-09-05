# ADR 0024: Provider-neutral skill orchestration

## Status

Accepted.

## Context

Agent Vesper already shipped a bounded local/global `SkillStore`, curated
release skills, skill bundles, lifecycle tools, and read-only workers. The
model still had to discover the catalog and call `read_skill` unaided, so
selection quality depended on the provider noticing those tools. TUI and ACP
could also drift because neither shared a pre-dispatch routing contract.

Claude Code's public Agent Skills documentation provides the behavioral
baseline: compact skill metadata is available for discovery, full instructions
load only when selected, users can invoke skills explicitly, model invocation
can be disabled, and `context: fork` runs in an isolated subagent. These public
contracts are used as product behavior, not as a claim about or copy of
proprietary ranking internals.

## Decision

1. `vesper-memory::skill_orchestrator` owns deterministic, provider-neutral
   metadata parsing, eligibility, ranking, conflict resolution, bounded
   composition, progressive loading, and outcome adjustment. Hosts never
   maintain their own skill tables.
2. Selection uses only bounded frontmatter/catalog metadata: name,
   description, tags, triggers, exclusions, file types, platform, required and
   allowed tools, conflicts, invocation policy, execution context, pin/archive
   state, side-effect class, and bounded verified outcome history. Skill body
   text never participates in ranking.
3. Automatic routing is conservative and chooses at most three skills.
   Explicit `/skill <name> [task]` and `/skill bundle:<name> [task]` routes are
   implemented and advertised by both production hosts. Missing or ineligible
   explicit selections fail before provider dispatch. Bundles are explicit
   only and their members are ranked within the same cap.
4. Eligibility fails closed for archives, user/model-only invocation rules,
   unsupported platforms, missing required tools, exclusions, conflicts, and
   implicit external-side-effect workflows. Selection never grants permission;
   the agent permission gate remains authoritative. `allowed-tools` narrows the
   model contract and cannot widen host authority.
5. Inline skill bodies are bounded per skill and in aggregate, injected only
   into the current provider request, and restored out of persisted history.
   `context: fork` bodies never enter the main conversation: the main model
   receives only identity and a directive to use the bounded read-only
   `delegate_task` worker, which must read the named skill in its own context.
6. Every production dispatch path runs the same router: TUI direct, non-ReAct
   VRO, and ReAct; ACP direct and non-ReAct VRO (ACP intentionally routes a
   ReAct profile through its tool-capable AgentLoop). Selected identities are
   observable and stored in the namespaced user-message extension for audit.
7. Compaction preserves bounded selected identities in the summary extension
   with `scope: compacted-audit` and `reactivate: false`. It never persists a
   skill body or blindly reactivates an old selection; the next user turn is
   independently rerouted against current policy, tools, platform, and task.
8. Successful and failed terminal outcomes feed a bounded, in-process score
   adjustment. Prompts, bodies, paths, and secrets are not retained. This
   signal breaks close ties but cannot override eligibility.

## Consequences

- Skill choice no longer relies on a provider first deciding to enumerate the
  catalog, and both hosts have identical automatic and explicit behavior.
- The local semantic scorer is deterministic and offline. No claim is made
  that it reproduces a proprietary embedding or ranking algorithm.
- Isolated skills require the real worker capability. A composition without
  `delegate_task` rejects them instead of leaking their body into main context.
- Catalog authors can improve routing through portable metadata without
  changing provider code. Existing skills with only name/description remain
  compatible.

## Verification

- `cargo test -p vesper-memory`
- `cargo test -p vesper-agent`
- `cargo test -p vesper-harness`
- `cargo test -p agent-vesper-tui --lib --bins`
- `cargo test -p agent-vesper-acp --lib --bins`
- `cargo xtask architecture`
- Workspace formatting, strict Clippy, and release-equivalent verification.
