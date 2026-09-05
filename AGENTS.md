# DOX framework

- DOX is highly performant AGENTS.md hierarchy installed here
- Agent must follow DOX instructions across any edits

## Core Contract

- AGENTS.md files are binding work contracts for their subtrees
- Work products, source materials, instructions, records, assets, and durable docs must stay understandable from the nearest applicable AGENTS.md plus every parent AGENTS.md above it

## Read Before Editing

1. Read the root AGENTS.md
2. Identify every file or folder you expect to touch
3. Walk from the repository root to each target path
4. Read every AGENTS.md found along each route
5. If a parent AGENTS.md lists a child AGENTS.md whose scope contains the path, read that child and continue from there
6. Use the nearest AGENTS.md as the local contract and parent docs for repo-wide rules
7. If docs conflict, the closer doc controls local work details, but no child doc may weaken DOX

Do not rely on memory. Re-read the applicable DOX chain in the current session before editing.

## Update After Editing

Every meaningful change requires a DOX pass before the task is done.

Update the closest owning AGENTS.md when a change affects:

- purpose, scope, ownership, or responsibilities
- durable structure, contracts, workflows, or operating rules
- required inputs, outputs, permissions, constraints, side effects, or artifacts
- user preferences about behavior, communication, process, organization, or quality
- AGENTS.md creation, deletion, move, rename, or index contents

Update parent docs when parent-level structure, ownership, workflow, or child index changes. Update child docs when parent changes alter local rules. Remove stale or contradictory text immediately. Small edits that do not change behavior or contracts may leave docs unchanged, but the DOX pass still must happen.

## Hierarchy

- Root AGENTS.md is the DOX rail: project-wide instructions, global preferences, durable workflow rules, and the top-level Child DOX Index
- Child AGENTS.md files own domain-specific instructions and their own Child DOX Index
- Each parent explains what its direct children cover and what stays owned by the parent
- The closer a doc is to the work, the more specific and practical it must be

## Child Doc Shape

- Create a child AGENTS.md when a folder becomes a durable boundary with its own purpose, rules, responsibilities, workflow, materials, or quality standards
- Work Guidance must reflect the current standards of the project or user instructions; if there are no specific standards or instructions yet, leave it empty
- Verification must reflect an existing check; if no verification framework exists yet, leave it empty and update it when one exists

Default section order:
- Purpose
- Ownership
- Local Contracts
- Work Guidance
- Verification
- Child DOX Index

## Style

- Keep docs concise, current, and operational
- Document stable contracts, not diary entries
- Put broad rules in parent docs and concrete details in child docs
- Prefer direct bullets with explicit names
- Do not duplicate rules across many files unless each scope needs a local version
- Delete stale notes instead of explaining history
- Trim obvious statements, repeated rules, misplaced detail, and warnings for risks that no longer exist

## Closeout

1. Re-check changed paths against the DOX chain
2. Update nearest owning docs and any affected parents or children
3. Refresh every affected Child DOX Index
4. Remove stale or contradictory text
5. Run existing verification when relevant
6. Report any docs intentionally left unchanged and why

## User Preferences

When the user requests a durable behavior change, record it here or in the relevant child AGENTS.md

## Project Contracts

- The frozen Python source at `/home/alex/Projects/Native GLM-5.2 Provider`
  remains a read-only behavioral oracle pinned to
  `bf4d4287e2e3320aa3f09015f678e6169d520045`.
- Production Rust crates follow the accepted ADRs under `docs/adr/`, use MSRV
  1.88, and must not claim GLM parity or multi-provider readiness before their
  migration gates.
- Never advertise invented providers, models, API plans, reasoning modes, or
  UI controls. Provider-specific values come from the owning real adapter and
  the frozen oracle; production currently registers Z.ai and LM Studio.
- Z.ai model metadata has one production source of truth in
  `vesper-provider-glm`; ACP and TUI must derive their model lists, limits, and
  capability gates from that catalog. Undocumented model-list endpoints or
  identifier-only discovery must not infer vision, reasoning, plan, or limit
  capabilities.
- “Multi-provider” means the provider-neutral registry/runtime architecture
  plus the real registered adapters. Z.ai and LM Studio are currently
  available; no additional provider may be claimed before it has
  authentication, catalog, transport, fixtures, and CI evidence.
- A feature may be called impossible or excluded only after checking the
  frozen oracle and current primary documentation and recording concrete
  technical evidence. Missing dependencies must fail truthfully; placeholders
  and mocked production behavior are prohibited.
- Production crates never depend on `vesper-testkit`, frontend crates, or
  disposable packages under `spikes/`.
- No live provider calls or user-state writes are permitted in foundation
  verification.
- Supply-chain license policy permits the OSI-approved `BSL-1.0` for native
  platform integrations such as the Windows clipboard backend; advisory,
  source, and wildcard-dependency gates remain fail-closed.
- Registry publishing follows the continuous-update contract: one open
  `agent-vesper` PR in `agentclientprotocol/registry`, updated in place on
  the same branch for every version bump. Never close-and-replace it.
- Public releases are exact-commit gated: push the version commit to `main`,
  require successful canonical, MSRV, and five-target foundation workflows
  for that commit, then create its immutable release tag. Never tag first and
  use the release matrix to discover platform failures.
- TUI↔ACP host parity is bidirectional: any host-agnostic capability or
  behavior change shipped in either host (cognitive memory, reasoning
  orchestration, streaming/finalization, tool/system-prompt behavior, or
  slash-command surface) MUST be evaluated and wired into the other host in
  the same change. Shared model-facing cognition instructions and the
  host-neutral slash-command catalog live in foundation crates and are
  enforced by cross-host registration/advertisement tests; composition
  adapters remain host-specific. Documented, justified host-specific exclusions
  (interactive-terminal, ACP-protocol, or browser-only UX) live in the
  affected app's nearest `AGENTS.md`.
- Provider-stream interruptions must preserve already-visible assistant
  output and session history. Automatic recovery is bounded and permitted
  only when no ambiguous tool-call fragment exists; neither host may replay a
  possibly side-effecting tool call.
- Context compaction is token-pressure driven against the active provider
  model's advertised window, transactional, and shared by direct, VRO, TUI,
  and ACP paths. It preserves system instructions and complete recent tool
  transactions, persists summary lineage/quality metadata, keeps the TUI's
  human-visible transcript intact, and fails closed before dispatch when the
  minimum safe suffix cannot fit. Manual `/compact [focus]` is semantic, not
  a message-count truncation control.
- Skill orchestration is provider-neutral and shared by TUI/ACP direct, VRO,
  and ReAct paths. It ranks only bounded metadata, applies fail-closed policy
  eligibility, composes at most three skills, injects inline instructions only
  for the active provider request, and keeps `context: fork` bodies inside a
  bounded worker. `/skill <name|bundle:name> [task]` is a cross-host explicit
  route. Selection never grants permission or external side effects; compacted
  identities are audit-only and must be rerouted before reuse.
- Users must not have to babysit an active native plan with repeated
  "continue" prompts. A normal provider stop or an ordinary iteration-segment
  boundary while plan items remain open triggers bounded autonomous
  continuation; only the ultimate safety ceiling may terminate unfinished
  work, and both hosts must surface that condition explicitly with the plan.
- Durable session checkpoints and lineage are OPT-IN in the ACP host:
  default OFF, enabled only by `AGENT_VESPER_ENABLE_CHECKPOINTS=1` or an
  explicit `AGENT_VESPER_CHECKPOINT_ROOT`. The auto-spawned ACP process
  must never create `.agent-vesper/` durable state in arbitrary project
  directories by default.
- Production harness work includes transactional sessions, provider-neutral
  runtime and agent loop, hosted tools, memory/checkpoints/MCP/plugins/workers,
  ACP composition, and the native TUI. Current status and evidence live in
  `docs/migration-status.md`; historical stage reports remain evidence, not
  current scope restrictions.

## Child DOX Index

- `docs/AGENTS.md` — documentation ownership, evidence standards, and child documentation boundaries.
- `fixtures/AGENTS.md` — language-neutral compatibility scenarios, schemas, and captured oracle results.
- `.github/AGENTS.md` — CI-only external platform validation workflows.
- `.cargo/AGENTS.md` — repository-local Cargo command and resolver policy.
- `crates/AGENTS.md` — production foundational crate boundaries and dependency direction.
- `apps/AGENTS.md` — thin production composition binaries.
- `registry/AGENTS.md` — ACP registry manifest for Zed discovery/install.
- `skills/AGENTS.md` — curated seed skill library bundled in release archives.
- `scripts/AGENTS.md` — cross-platform installers for the `agent-vesper-acp` binary.
- `spikes/AGENTS.md` — disposable Rust compatibility and platform experiments.
- `tools/AGENTS.md` — non-production migration tooling and oracle ownership.
- `xtask/AGENTS.md` — repository verification, fixture, architecture, and MSRV commands.
