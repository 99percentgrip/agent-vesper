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

- Requested microphone UI: keep a visible bottom-panel red circle “Push to talk”
  toggle that becomes a red square “Stop” while recording. One click starts, one
  click stops; preserve F5, truthful recorder state and responsive input. Support
  long dictation until Stop, with elapsed time, responsive transcription progress
  and explicit retry/discard after errors rather than silently losing the audio.

- Optional dependency setup belongs in native Settings: detect healthy local
  runtimes, offer confirmed setup/repair, show real progress and verify readiness.
  Beginners should not need normal configuration-file editing or package commands.
  Preserve existing runtimes/workloads; OS approval and reboot requirements remain
  explicit. Core coding stays usable when optional setup is declined.

- Include documentation, PRD/status, owning DOX and evidence-index updates in
  the implementation/release candidate commit before CI. Do not automatically
  push a separate documentation-only commit after a green release. Keep receipts
  that can only exist after publication in the owned local `docs/foundation/`
  report and deliver them; publish those updates with the next authorized code
  change unless Alex explicitly requests a documentation-only push.
- Prose-only and execution-report updates require content, links, JSON and
  whitespace checks, not program suites, a version bump or another release.
  Automatic CI filters must exclude prose/report-only pushes while retaining
  gates for source, fixtures, bundled skills, dependencies, workflows and
  release-objective provenance. Never represent skipped checks as release proof.

When the user requests a durable behavior change, record it here or in the relevant child AGENTS.md

- Keep the GitHub README a concise user-facing front page: capabilities, install,
  first use, dependencies, uninstall, and a clear documentation link. Put detailed
  setup and commands in user guides, and keep PRDs, implementation milestones,
  internal identifiers and historical repair narratives in engineering documents.
  Prefer concrete supported behavior over inflated guarantees or stale counts.
- When a complete repair or implementation is requested, continue across
  ordinary milestones until its required code and acceptance work are done.
  Do not substitute a partial checkpoint or a release for completion.
- Every completed work unit (PR, repair, recon mission, or directive)
  ends with BOTH a formal execution report in `docs/foundation/` following
  the house convention (objective, methods/commands, files, exact
  evidence, deviations, unresolved items, readiness effect), linked from
  `evidence-index.md` and the owning PRD, AND a full summary presented in
  the conversation at delivery. Neither alone closes a work unit.
  (Productized: both hosts inject the shared
  `vesper-harness::COMPLETION_REPORTING_INSTRUCTION` mandate and the
  `work-unit-reporting` seed skill ships in the library — installed
  agents follow this rule by default, not just in this workspace.)
- PRD completion claims must trace every required behavior to current,
  scope-appropriate evidence. Model-written plan checkmarks, passing unrelated
  tests, and another agent's assurance are not completion evidence. Preserve
  missing, failed, stale, and unexecuted acceptance items in reports; never
  silently weaken the scope to obtain a completed status. The native
  enforcement contract is ADR 0028; implementation evidence and limitations live
  in `docs/foundation/completion-assurance-execution.md`. Run `cargo xtask
  acceptance` before claiming changes to this gate are verified.
- Feature activation belongs in native `/settings` controls, including
  Settings → Web tools with persisted on/off choices. Manual configuration
  file editing must not be the normal activation workflow.
- Required web-driver images belong in the installation package. Installers
  and native Settings setup/repair must handle their verified import; users
  must not have to locate separate driver release assets.
- Provider selection belongs in Settings and uses the same restrained,
  bordered Save/Cancel menu style as Web tools, not a separate legacy picker.
- `/usage` is a standard provider-neutral status card in both hosts: active
  model/reasoning/permissions, estimated context, and provider-reported account
  windows, remaining allowance, and resets. New providers implement the shared
  usage port or report explicitly unavailable values; never fabricate quotas.
  Model and reasoning menus must use adapter-owned per-model/auth-mode metadata.
  Its terminal presentation uses clean aligned rows and solid progress bars;
  raw ASCII-art borders and hash-mark meters are not acceptable.
- The requested OpenAI integration must expose one provider with API-key and
  ChatGPT subscription authentication choices in native Settings and retain
  the shared harness features in both hosts. Both authentication modes must
  run natively in Vesper without installing, bundling, or launching Codex CLI
  or app-server. Do not advertise OpenAI before its gates pass.
  Switching to another provider and back must reuse the selected valid OpenAI
  credential; only explicit sign-out, credential failure, or `/auth` may
  request authentication again.

- Ordinary native Settings use one draft and a Save changes / Discard changes /
  Keep editing prompt on exit. Provider setup may retain explicit immediate saves.
  Every Settings panel follows the selected theme. Saved execution choices must
  reach the next coding turn and be restored with adapter validation on restart.
- Enforced completion recognizes the task's PRD and remembers its path after
  native opt-in; manual Settings path entry is optional. Automatic selection cannot
  weaken requirement coverage, independent review, or evidence enforcement.
- Check for updates offers a confirmed installer workflow with actual progress,
  failures and restart guidance, rather than only linking to release downloads.

- Native output uses distinct semantic colors for activity labels, commands and
  source tokens. Tool dots reflect actual outcomes: green success, blinking orange
  while running, red failure; missing completion never implies success. Reports
  share a consistent left edge with hanging list indents and responsive tables.
- Commit/push/version and release work does not authorize replacing Alex's local
  installation. Leave native update testing to Alex unless he explicitly requests
  installation; do not run an installer merely to verify a release.
- Future providers stay provider-neutral and follow the same path as the
  current registered adapters. The contract is in Project Contracts below.

## Project Contracts

- Hash-pinned fixtures and routing evidence retain LF bytes through `.gitattributes`
  on every platform. Do not weaken frozen digests to accommodate checkout conversion.

- RRC local verification uses its controller-owned Host Resource Governor: Linux
  physical/cgroup memory, swap, owned process-tree and target-filesystem observations
  preserve a desktop reserve; every Cargo path inherits bounded concurrency and a
  managed target cache; expensive compiler gates serialize; and critical pressure
  reaps only the owned group while preserving a resumable local epoch. TUI and ACP
  show observed resource values/actions, never fabricated host health.

- The frozen Python source at `/home/alex/Projects/Native GLM-5.2 Provider`
  remains a read-only behavioral oracle pinned to
  `bf4d4287e2e3320aa3f09015f678e6169d520045`.
- Production Rust crates follow the accepted ADRs under `docs/adr/`, use MSRV
  1.88, and must not claim GLM parity or multi-provider readiness before their
  migration gates.
- Never advertise invented providers, models, API plans, reasoning modes, or
  UI controls. Provider-specific values come from the owning real adapter and
  its evidence sources; production registers Z.ai, LM Studio, native OpenAI,
  and native xAI / Grok.
- Z.ai Coding Plan MCP integrations are scoped to Vesper's `zai` provider.
  This is Vesper's provider-isolation rule, not a claimed vendor protocol
  restriction. Provider ID `xai` means xAI / Grok and never grants Z.ai tools.
  Preserve independent MCP servers, native web tools, and provider-owned search.
- Z.ai model metadata has one production source of truth in
  `vesper-provider-glm`; ACP and TUI must derive their model lists, limits, and
  capability gates from that catalog. Undocumented model-list endpoints or
  identifier-only discovery must not infer vision, reasoning, plan, or limit
  capabilities.
- “Multi-provider” means the provider-neutral registry/runtime architecture
  plus the real registered adapters. Z.ai, LM Studio, OpenAI, and xAI are the
  registered adapters; no additional provider may be claimed before it has
  authentication, catalog, transport, fixtures, and CI evidence.
- A future provider must use the same provider-neutral path as the current
  registered adapters. Authentication, catalog, model/reasoning/plan controls,
  transport, Settings, `/auth`, and startup sign-in come from that provider's
  descriptor and ports (`ProviderCredentialPort`, catalog, and superpower
  policy). The TUI and ACP must not grow a provider-name match arm or a direct
  concrete-provider import to add it. Registering the adapter is what makes
  the existing harness behavior apply. Missing behavior is a gap in the
  adapter or the shared port, not a license for a one-provider shortcut.
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
- Use standard public GitHub Actions for this public project. Deliberately
  failing RRC workflow/publication acceptance belongs in a separate public test
  repository; a private-runner billing prerequisite must not be introduced.
- Public releases are exact-commit gated: push the version commit to `main`,
  require successful canonical, MSRV, five-target foundation, and contained
  web-driver image acceptance workflows
  for that commit, then create its immutable release tag. Never tag first and
  use the release matrix to discover platform failures.
- Release and CI recovery progression is owned by the provider-neutral Release
  Recovery Controller in `vesper-harness`, not a free-form `/release` prompt.
  Wait for complete exact-SHA matrices, capture and fingerprint first causal
  failures, require focused proof plus a relevant state change, and enforce the
  persisted retry budget. RRC must autonomously diagnose and repair admitted
  source/CI failures, recover safe interruptions and finish registry/report delivery
  through the shared provider-neutral path in both hosts. Repeated activity without
  evidence must stop truthfully within its persisted limits; users must not need
  corrective prompts to advance ordinary stages. Use the invoking native release
  route for ordinary release work; do not substitute a disposable ACP driver with
  independent default permissions. An explicit ACP client must service permission
  requests while the native release prompt is active and treat its final unfinished
  response as stopped work, rather than continue polling a failed epoch. Never
  equate local fixes or a passing old release with the installed build containing
  those fixes. Audit confirmed gaps before the
  next version bump and keep repair documentation in that release candidate.
  A new natural-language release request automatically
  reconciles a persisted epoch: resume the matching recoverable objective from its
  safe stage, archive and supersede an obsolete same-objective prerelease candidate,
  and ask one bounded human clarification for unrelated or irreversible state.
  Epoch IDs, internal states, ledger paths and manual resume/cancel commands are
  diagnostics, not normal admission prerequisites. While an RRC task is active,
  hosts project bounded live telemetry from controller-owned gate, subprocess and
  output state, never a fabricated percentage; elapsed/quiet time ticks, child and
  gate changes remain visible, and idle labels are forbidden. Interactive Ratatui output
  has one writer: release children use concurrently drained pipes, terminal controls and
  secrets are removed before bounded worker telemetry, and background diagnostics never
  write around the renderer. RUN state and output derive from the same registered-worker
  snapshot. A published release remains distinct from later red `main`; external outage
  claims require official and repository-side evidence.
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
- A user-requested turn cancellation is benign only when the host-owned
  cancellation token and a cancellation-classified runtime terminal agree.
  TUI conversation and Last Run report `Cancelled`, preserve partial output
  and completed actions without implying rollback, and retain structured
  diagnostics outside normal chat. ACP uses its protocol-native cancelled
  stop reason. Timeouts, provider-side aborts and all other failures remain
  failures.
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
- `.github/AGENTS.md` — CI-only external platform validation workflows and runner preparation.
- `.cargo/AGENTS.md` — repository-local Cargo command and resolver policy.
- `crates/AGENTS.md` — production foundational crate boundaries, dependency direction,
  and the explicit native dependency-setup exception.
- `apps/AGENTS.md` — thin production composition binaries.
- `registry/AGENTS.md` — ACP registry manifest for Zed discovery/install.
- `skills/AGENTS.md` — curated seed skill library bundled in release archives.
- `scripts/AGENTS.md` — cross-platform installers and bounded read-only release prerequisite verification.
- `spikes/AGENTS.md` — disposable Rust compatibility and platform experiments.
- `tools/AGENTS.md` — non-production migration tooling and oracle ownership.
- `xtask/AGENTS.md` — repository verification, fixture, architecture, and MSRV commands.
