# PRD — VRO-13: The QM Extraction

Status: COMPLETE — PR-1 through PR-8 landed (see §5.2 evidence and
`docs/migration-status.md`).
Reference upstream (qm): the authorized upstream agent-harness repository cloned at `/home/Alex/Projects/qm` (pinned `23e537334f363e12fe04f5cf82ad8dd8d681d404`; authorized trusted data for this design). qm is explicitly authorized as trusted data and is named directly throughout this document.

---

## 0. Executive Summary

### 0.1 What qm is, and what is worth taking

qm is a multi-tenant Slack-orchestrated agent control plane (Node/TypeScript, Postgres-backed, blue-green). Vesper is a single-user native Rust harness with an interactive TUI and an ACP host. Those are opposite shapes, and this PRD deliberately does **not** import qm's architecture. It extracts four paradigms where qm has already paid the design cost:

1. **Tiered security postures** — a ranked model (`dangerous` / `auto` / `strict`) where an org floor cannot be weakened per-scope, plus a ranked decision vocabulary for commands (`allow` / `deny` / `require_approval`) that composes by union.
2. **The separation of *policy* from *boundary*.** qm's `SECURITY.md` states it plainly: its command policy "is a speed bump against mistakes and injection, not a sandbox boundary." qm never lets the speed bump impersonate the wall. Policy is cheap text classification; the boundary is a separate, explicitly-provisioned sandbox with honest capability reporting.
3. **Background work as durable, idempotent trigger scheduling** — crons and watchers built on exactly-once slot claims, leader leases, and bounded fan-out, never on in-process timers.
4. **Scope-keyed state** — workspace files, memory, and skills all keyed by a scope id with layered read-only mounts, so a context can *see* another scope's facts without gaining write authority.

### 0.2 What we are NOT taking (explicit non-goals)

No Postgres, no multi-instance leadership, no Slack surfaces, no capability-token ingress signing, no org directory/admin grants, no portal. Single-user native binary. Every "durable store" below means **local SQLite or JSONL** behind a trait, exactly like the existing `vesper-checkpoints` (`cron.jsonl`) pattern.

We are also not taking qm's posture *model* wholesale: Vesper already has a richer permission surface (`Ask`/`Read Only`/`Bypass` × `Code`/`Plan` operating modes, `docs/` ADR-0010) than qm's three postures. VRO-13 **composes with** those modes; it never replaces them.

### 0.3 Non-negotiable: zero harness degradation

Every feature ships as an **opt-in module behind a default-off switch**. The existing contract:

- The **1,236 test floor** (`docs/result-aware-loop-detection-audit-report.md:259`; floors 1,193 workspace + 336 TUI) must not drop, and no test may be deleted or weakened to admit a feature. Every PR must show floor ≥ its predecessor on its touched crates.
- **VRO-12 loop detector** (`crates/vesper-agent/src/vro/loop_detector.rs`) remains unconditionally active: `LoopDetector::new()` stays call-site mandatory in both `agent_loop.rs` and `vro/react.rs`, and Z1/Z2/Z3 tests (`loop_detector_tests.rs`) remain byte-identical.
- **TUI responsiveness**: no feature may take an `await` point on the render path, and no host task may hold a lock across an `.await` that the interactive loop needs. Background daemon features own their own task/tokio runtime scope and are verified by a new **input-latency regression gate** (§5.5).
- **Bypass stays fast**: the bypass path gains exactly one unsynchronized read (an `Arc<OnceLock>` status check plus a compiled-ruleset pointer dereference) — no regex execution, no allocation, no I/O. Bypass must never *escalate* authority: `PolicyEffect::Deny` already outranks Bypass and VRO-13 adds no new rule that could change that ordering (`crates/vesper-policy/src/lib.rs:177`, `fixture_bypass_plus_deny_is_absolute`).

### 0.4 Source-of-truth map (qm → Vesper seam)

| qm concept | qm location (pinned rev) | Vesper seam today |
|---|---|---|
| 3 postures + floor composition | `src/security/security-posture.ts` | `SessionPermissionMode` (Ask/ReadOnly/Bypass) + `PolicyEffect` |
| Command denylist + shell normalizer | `src/policy/command-policy.ts` (911 L) | none (gap) → **F1** |
| Hard-denial precedence (deny > approval > allow) | `src/policy/command-policy.ts` `evaluateCommand` | `PolicyEvaluator` (`vesper-policy`) |
| Sandbox trait + fail-closed caps | `src/sandbox/sandbox.ts` | `SandboxCapabilities`/`IsolationRequirement` (`vesper-security`), no backend (gap) → **F2** |
| Per-scope routing, capability refusals | `src/sandbox/sandbox-routing.ts` | `PolicyEvaluator` `IsolationUnavailable` |
| Durable cron, exactly-once slot claim | `src/cron/scheduler.ts`, `cron-store.ts` | `CronRegistry` (`vesper-checkpoints/src/cron.rs`) |
| Watchers/monitors, literal patterns, heartbeat | `src/monitors/monitor-poller.ts` | none (gap) → **F4** |
| Scope-keyed workspace/skills/memory | `src/workspace/workspace-store.ts`, `src/skills/skill-store.ts` | ADR-0021 project/global cognition split |
| RO layer fingerprint + tar materialize | `src/sandbox/ro-layers.ts` | none → **F3** (skills materialization only) |

### 0.5 Definition of done per feature

Each feature has: (a) a pure module in the owning crate with unit tests; (b) an integration test against a fixture under `fixtures/`; (c) a default-off or explicitly-confirmed runtime flag; (d) a doc note in the nearest `AGENTS.md`; (e) a verification line for `docs/AGENTS.md` evidence standards.

---

## 1. Feature 1 — Hard Denial Firewall

### 1.1 Problem

Today a shell-class tool call in Bypass mode runs whatever the model emits. `run_command` (`crates/vesper-runtime/src/registry.rs`, `RunCommand::execute` → `run_bounded`) applies no content policy at all: Bypass means "no approval prompts," which today implies "no floor." That is a real hole for an agent whose primary surface is a single-user laptop.

qm hit the same problem and solved it as **two separate questions**: (1) *is this command text shaped like destruction?* (policy) and (2) *will this execution be contained?* (boundary). VRO-13 F1 answers only (1).

### 1.2 Design — `vesper-policy` gains a `CommandFirewall`

New pure module `crates/vesper-policy/src/firewall.rs`:

```
pub struct CommandRule { pub pattern: RulePattern, pub decision: RuleDecision, pub reason: &'static str }
pub enum RuleDecision { Allow, Deny, RequireApproval }
pub struct CommandFirewall { rules: Arc<[CommandRule]>, anchored: CompiledRules }
pub struct FirewallVerdict { pub decision: RuleDecision, pub matched_rule: Option<usize>, pub scan_text: String }
```

- **Default ruleset** (compiled once, lazy static; modeled on qm's `ORG_FLOOR_RULES`, adapted to a single-user laptop):
  - `deny`: `mkfs`, fork-bomb shape `:(){ :|:& };:`, `dd of=/dev/sd*`, writes to `/dev/mem` or raw disks.
  - `deny`: recursive deletes outside the workspace root — `rm` with `-r`/`-f`/`--recursive` where the target resolves to `/`, `$HOME`, or any path escaping the primary root (confine check reuse).
  - `deny`: `chmod -R 777 /`, `chown -R` on `/`.
  - `require_approval` in Ask mode (ignored in Bypass, per §1.3): `git push --force`, `drop|truncate table`, `curl|wget … | sh/bash`, `dd` on any block device, `git reset --hard`.
- **Config**: `AGENT_VESPER_FIREWALL` env (`on` default from Phase 1 of rollout; see phasing) or `firewall.toml` in the state root with the same schema as qm's `parseCommandPolicy` (mode `denylist`|`allowlist`, rules array) — parsed and validated once at startup, fail-closed on invalid regex via the safe-regex discipline (qm's `compileSafeRegex`).
- **Evaluation order**: compile → normalize (§1.2.1) → first-match by declared rule order (qm semantics) → `deny` short-circuits absolutely; `require_approval` short-circuits only against the existing approval broker.

#### 1.2.1 Shell normalization before matching (the whole game)

qm's insight is that naive regex-on-raw-text is trivially bypassed (`"rm" -rf ~`, `r\m`, heredocs, `$(…)`, base64|sh). Its `scannableCommand` pipeline:

1. strip heredoc bodies *unless* the heredoc feeds an interpreter (`psql`, `python`, `sh -s`),
2. unquote/unescape bare words, decode ANSI-C `$'…'`,
3. keep command substitutions as separate scannable payloads, recursing to depth 8,
4. segment pipelines (`|`, `;`, `&&`, `||`) so `curl … | sh` matches the pipe-to-shell rule across segments.

We port that pipeline faithfully to Rust with one deliberate divergence: **we keep qm's heredoc-interpreter detection list** (`psql|mysql|mariadb|sqlite3?|sqlcmd|python|node|perl|ruby` and shells without `-c`) but **we do not attempt qm's full `scanShell` word-splitter** (its ~400-line tokenizer handling `env -S`, `command`, `exec`, `sudo` option parsing). Instead we normalize then split on shell metacharacters with the tokenizer surface the `regex` crate can handle, and we document the residual bypass surface honestly (§1.5). Rationale: a single-user harness where Bypass mode is an explicit user choice does not need the full adversarial tokenizer to be useful; it needs the common-case killers caught, and honest docs that the firewall is defense-in-depth, not containment. This is the same honesty qm's SECURITY.md applies to itself.

Normalization lives in `vesper-policy::firewall::normalize` as a pure function so it is unit-testable without I/O.

#### 1.2.2 Integration — one choke point, no TUI duplication (PR-2, not this PR)

Wire exactly once, in the **ToolInvoker/executor path**, not the TUI:

- `vesper-runtime` `registry.rs`: `RunCommand::execute` and any `run_shell`-class executor call `firewall.scan(&command)` before `run_bounded`. A `Deny` verdict returns a `ToolError::Failed` whose text is model-facing guidance (mirroring the VRO-12 deny text shape: `[VRO-13 Firewall] denied: {reason}; matched: {rule}`), so the model can recover by choosing a safer command. This is the same pattern the loop detector uses to stay in-loop instead of hard-failing the turn.
- The **TUI's `/permission` toggle is untouched.** Firewall is not a permission mode. `/permission bypass` continues to mean "no prompts"; the firewall is a content floor that composes *under* it. The `/status` panel gains one read-only line (`Firewall: on/off`) and a `/firewall` command offers view/disable-with-restart only — mirroring qm's portal-only rule that gate-changing decisions live outside the agent's own reach.
- **ACP host parity** (Project Contract, host parity rule): the same `vesper-policy` firewall instance is compiled once and shared by both hosts. The ACP adapter's tool gateway calls the same `firewall.scan` via the shared runtime registry; no host-local copy exists. Cross-host enforcement is tested by one registration test asserting the shared instance id.

#### 1.2.3 Bypass stays fast

When `permission_mode == Bypass` and the firewall is off (or its ruleset is empty), `scan` is a single `Arc` clone-free check: one relaxed load of a `OnceLock<FirewallStatus>` plus a length check. Budget: **≤ 200 ns and zero heap allocation** for the off/fast path, enforced by a `cargo bench`-style unit assertion using `std::hint::black_box` and a cycle-count bound (or, if flaky on CI, a structural no-allocation test via `#[global_allocator]` counting). In Ask mode with rules enabled, evaluation cost is bounded by ruleset size (default 11 rules) over normalized text; documented, not benchmarked to a hard number.

### 1.3 Interaction with existing permission modes

| Mode | Firewall Deny | Firewall RequireApproval | Firewall Allow |
|---|---|---|---|
| Ask | Deny (absolute) | Ask via existing broker | existing policy path |
| Read Only | Deny (absolute) | moot (Shell class already denied) | existing policy path |
| Bypass | **Deny (absolute)** | **skipped** (Bypass = no prompts) | straight to exec |
| Plan | n/a (Shell not permitted in Plan) | n/a | n/a |

`PolicyEffect::Deny` precedence is untouched; the firewall emits `PolicyEffect::Deny` into the existing `PolicyInput.policy` field for shell-class operations rather than adding a new bypass-respecting rank. Concretely: the executor-side scan result maps `RuleDecision::Deny → PolicyEffect::Deny`, `RequireApproval → PolicyEffect::Ask`, `Allow → unchanged`, so `PolicyEvaluator::evaluate` continues to own all precedence and the fixture matrix in `vesper-policy/src/lib.rs` tests stays authoritative. No new decision enum values.

### 1.4 What F1 explicitly does not do

- No prompt-injection classifier (qm's `security-screener` LLM pass) — out of scope; the TUI already carries trust judgments.
- No audit sink beyond the existing session transcript. Vesper has no org audit story and should not pretend to.
- No protection against `python -c '…'`, base64-encoded payloads, or writing a script then executing it. Documented residual risk (§1.5), covered only by F2 when sandboxing is on.

### 1.5 Residual risk (documented, not hidden)

The firewall is text classification. It is defeated by: obfuscated interpreters (`python -c`, `perl -e`), encoded payloads, two-step write-then-exec, path confusion via symlinks resolved after normalization, and generated scripts. This is exactly qm's own stated limitation. F1's contract with the user is: **catches accidents and the obvious; never advertised as containment.** The `/firewall` panel and the PRD-linked doc must carry this sentence verbatim.

### 1.6 Acceptance criteria

1. `vesper-policy::firewall` unit tests: default ruleset blocks `rm -rf /`, `rm -rf ~`, `mkfs.ext4 /dev/sda1`, `chmod -R 777 /`, fork bomb, `dd if=… of=/dev/sda`, quoted variants (`"rm" -rf /`, `r\m -rf /`), `$'rm' -rf /`, heredoc-fed `psql` with `drop table`, and `curl … | sh` across a pipeline.
2. Normalization recursion: `$(rm -rf /)` inside a quoted string is denied.
3. Bypass-mode tests: Deny verdicts still deny; `RequireApproval` never prompts in Bypass; fast-path no-allocation test green.
4. All 1,236-floor tests still pass; `vesper-policy` test count strictly increases.
5. `cargo xtask architecture` passes (no forbidden dependency direction; `vesper-policy` stays dependency-light: `regex`, `serde`, `vesper-domain`, `vesper-security` only).

---

## 2. Feature 2 — Opt-in Sandboxing

### 2.1 Problem

`IsolationRequirement` and `SandboxCapabilities` already exist in `vesper-security` as a fail-closed capability contract — and there is **no backend**. Today the only isolation a Vesper tool call gets is process-tree cleanup inherited from `run_bounded` (ADR-0005 descendant-reaping). Any tool that *demands* isolation gets `IsolationUnavailable` denial, which is honest but means the contract is unexercised.

qm's answer is a `Sandbox` trait with multiple backends (Docker local, Fly sprites, AWS microVM) behind a router that refuses capabilities a backend lacks, plus per-scope persistent home volumes and RO-layer mounts.

### 2.2 Design — `vesper-sandbox` crate, two backends, honest caps

New crate `crates/vesper-sandbox` (production crate, follows `crates/AGENTS.md`):

```
pub trait SandboxBackend: Send + Sync {
    fn capabilities(&self) -> SandboxCapabilities;                    // vesper-security types
    async fn provision(&self, spec: &SandboxSpec) -> Result<SandboxHandle, SandboxError>;
    async fn run(&self, handle: &SandboxHandle, argv: &Argv) -> Result<ExecOutput, SandboxError>;
    async fn teardown(&self, handle: SandboxHandle, mode: TeardownMode) -> Result<(), SandboxError>;
}
```

- **Backend A — Linux namespaces** (primary, zero-dependency): `unshare`/`clone` with `CLONE_NEWNS | CLONE_NEWPID | CLONE_NEWNET` (optional `CLONE_NEWUSER` per config), bind-mounting the primary root read-write and `/usr`, `/bin`, `/lib*` read-only, via `nsenter` to a short-lived supervisor. Implemented with `std::process` + `nix`-free direct syscalls where possible (Linux-only, `#[cfg(target_os = "linux")]`; other platforms report `CapabilityStatus::Unavailable` honestly and `IsolationRequirement::Full` demands fail closed, exactly as `vesper-security`'s `satisfies()` already specifies).
- **Backend B — Docker** (opt-in, feature `docker`): wraps `docker run --rm` with cpu/memory limits, workdir bind-mount of the primary root, no network unless explicitly granted, image allowlist from config. Modeled on qm's `local-sandbox.ts` container naming (`…-sbx-<scope>`) but Vesper's is `agent-vesper-sbx-<session-id-slug>`; **cold-start guard**: provision fails fast with a model-facing "sandbox unavailable; the operation needs isolation" error rather than hanging the turn — qm's `CapabilityUnsupportedError` refusal shape.
- **Caps are probed, never assumed.** Each backend self-reports via `capabilities()`; a namespace backend that failed to unshare network reports `network: Unavailable`, and `PolicyEvaluator` then denies `IsolationRequirement::Network` demands. This reuses `vesper-security` verbatim — zero new types.

### 2.3 When sandboxing engages (opt-in only)

Three triggers, all explicit:

1. **Tool demands it**: a tool's `ToolDefinition` gains optional `required_isolation: IsolationRequirement` (already reserved in domain via `PolicyInput.required_isolation`). F1's Deny rules for out-of-root recursive deletes become moot when the shell tool is instead run sandboxed: `run_command --isolation filesystem` style argument, or a per-project `.agent-vesper/config.toml` `[sandbox] filesystem = true` scope demand. Default is and remains `None`.
2. **Scope demands it** (F3): a workspace scope can declare `[sandbox]` requirements that apply to all shell/process tools in that scope.
3. **User demands it**: TUI `/sandbox on|off|status` sets a session preference, surfaced in `/status`, mirrored to ACP session controls (host-parity rule). Not a permission mode.

No trigger ⇒ **zero cost**: the registry does not construct a backend unless a demand exists; the capability lookup is a `OnceLock`-cached probe. The single-user invariant holds: no auto-sandboxing of ordinary commands, ever — the default path stays byte-identical to today.

### 2.4 What runs inside

Only `run_process`/`run_command`-class executors route through a backend, and only when demanded. File tools (`read_file`, `write_file`, `edit_file`, `apply_patch`) keep direct FS access because they already confine to the primary root (`confine()` in `vesper-runtime/registry.rs`); sandboxing them would add latency without adding a boundary the confine check doesn't already provide. This is a deliberate divergence from qm, which sandboxes all harness work because it is multi-tenant; Vesper is single-user and its file tools are already path-confined.

Credential hygiene follows qm's lesson (SECURITY.md "sandbox credentials are plaintext while in use"): **no secrets, tokens, provider keys, or the global cognition root are ever provisioned into a sandbox.** The sandbox env is a fixed allowlist (`PATH`, `HOME` → scratch home, `LANG`, `TERM=dumb`), plus non-interactive discipline ports of qm's `NONINTERACTIVE_ENV` (`PAGER=cat`, `GIT_TERMINAL_PROMPT=0`, `DEBIAN_FRONTEND=noninteractive`). Provider auth stays in the harness process.

### 2.5 Acceptance criteria

1. `vesper-sandbox` unit tests on Linux CI: namespace backend provisions, runs `id -u` inside, teardown reaps; Docker backend tests are `#[ignore]`-gated on `DOCKER_AVAILABLE` env (no CI dependency, honest failure text when absent).
2. Capability honesty: a stub backend claiming only `process_tree` fails `Filesystem` demands — port of `vesper-security`'s existing test shape.
3. Opt-in zero-cost test: with no isolation demand, `RunCommand::execute` byte-identical behavior; new tests assert the registry does not construct a backend when nothing demands isolation.
4. No new production dependency on `vesper-testkit`, frontend crates, or `spikes/` (Project Contract). `nix` is not added; syscalls via `std::os::linux` + `libc` (already in tree) or a bounded `syscalls` audit if unavoidable.
5. Fixtures under `fixtures/sandbox/`: namespace isolation scenario (write outside root fails), docker-unavailable honest refusal.

### 2.6 Explicit non-goal

No egress proxy, no per-scope persistent volumes, no image fingerprinting, no scratch-sandbox credential links, no blob staging. Those exist in qm because it is a hosted multi-tenant service. Vesper's single-user sandbox is ephemeral: provision → run → teardown, state lives in the workspace, not the sandbox.

---

## 3. Feature 3 — Scoped Workspaces

### 3.1 Problem

Vesper's cognitive memory is already split project/global (ADR-0021), but **loaded `.md` skills** and the firewall/sandbox configuration are session-global: a skills directory is whatever the TUI/ACP host found, with no per-directory isolation, and nothing stops project A's `.agent-vesper/skills/` from leaking into project B's context.

qm scopes everything — workspace files, skills, memory — by scope id, with RO layers letting a session *read* an org-level skill without writing to it.

### 3.2 Design — scope resolution at host boot, not in the loop

New pure module `crates/vesper-agent/src/vro/scope.rs` (VRO module, sits beside `learning.rs`/`lens_integration.rs`) + host wiring:

```
pub struct WorkspaceScope {
    pub id: ScopeId,                  // canonical absolute path hash
    pub root: PathBuf,                // the project directory
    pub state_dir: PathBuf,           // .agent-vesper/ under that root (unless overridden)
    pub cognition: ScopedCognition,   // project engine per ADR-0021
    pub skills: ScopedSkills,         // per-scope skill source
    pub firewall: Option<FirewallConfig>,   // per-scope overrides (F1)
    pub sandbox: Option<SandboxSpec>,       // per-scope demands (F2)
}
```

- **Scope identity** is the canonicalized absolute path of the working directory (qm uses principal/channel ids; Vesper's single-user equivalent is the directory). `ScopeId` is a stable short hash so stores key on it, not on the path string (port of qm's `scopeStorageKey`).
- **Layered resolution** (qm's `WorkspaceLayer` model, simplified to single user):
  - Layer 0 (always, RW): the project's own `.agent-vesper/`.
  - Layer 1 (always, RO): the user's global `~/.local/share/agent-vesper/` (global cognition + bundled seed skills, already the ADR-0021 global root and `skills/AGENTS.md` seed library).
  - Layer 2 (opt-in, RO): explicit additional paths via `AGENT_VESPER_EXTRA_SCOPES` — the single-user stand-in for qm's cross-scope grants.
- **First-write-wins by layer order**, reads union in layer order (project overrides global), mirroring qm's `composePolicy`/`composeSecurityPosture` floor semantics: a project can be *stricter* than global, never weaker. So a project may deny a command the global config allows, but may not un-deny a global deny. Implementation: per-scope firewall config composes as `global_rules ∪ project_rules` with deny precedence — same union-then-rank as qm's `composePolicy`.
- **Skills**: `ScopedSkills` loads `.md` skills from `state_dir/skills/` with safe-name and safe-path validation (port of qm's `safeSkillFilePath` — reject absolute, `..`, NUL). Global seed skills remain read-only. No cross-project skill leakage: resolution is per-scope at host boot; the registry is rebuilt per session start, not cached globally.
- **Memory**: ADR-0021 already provides the split; F3 only binds the project engine to `WorkspaceScope.cognition` so the TUI and ACP host derive the engine from the same resolved scope (one source, two hosts — host-parity rule).

### 3.3 What changes for the running loop

Nothing. `WorkspaceScope` is resolved once at host boot and handed to the registry as plain configuration. The ReAct loop (`vro/react.rs`, `agent_loop.rs`) never sees scopes; it sees the same `ToolContext` it sees today, with `primary_root` already the project root. This is the zero-degradation guarantee structurally: scopes are a host-layer concept, and the loop layer is untouched.

### 3.4 Acceptance criteria

1. Pure `scope.rs` tests: canonical path → stable `ScopeId`; layer union with deny-precedence composition; safe-path rejection (`../`, absolute, NUL); project-stricter-than-global composition; project cannot weaken a global deny.
2. Two-directory isolation test: TUI boots in dir A, skills from dir B are absent from the advertised tool surface; and vice versa.
3. ADR-0021 tests (`scoped_cognition_commands_parse_overrides_and_lifecycle_operations`, `smart_memory_routing…`, `promotion_and_demotion…`) remain green and unchanged.
4. `AGENT_VESPER_EXTRA_SCOPES` is opt-in; default behavior byte-identical to today (one test asserting no extra layer is mounted by default).
5. Both hosts derive from the same resolved scope: one cross-host test asserts TUI and ACP resolve identical `ScopeId` for the same directory.

---

## 4. Feature 4 — Background Daemon Mode

### 4.1 Problem

The TUI already knows it isn't a daemon: `commands.rs:417` documents `/loop` as "register a bounded cron entry (the TUI is not a daemon…)". Cron *fire* today happens inside the TUI's own tokio task (`main.rs` `run_cron_scheduler`), so scheduled work dies when the terminal closes, and watchers don't exist at all. Meanwhile the TUI must remain perfectly interactive — it's the primary surface.

qm runs crons and monitors as always-on pollers with leader leases, exactly-once slot claims, fan-out caps, and rate limits, feeding results back as trigger turns.

### 4.2 Design — same binary, new long-lived mode

New composition binary behavior in `apps/agent-vesper-acp` and a new TUI subcommand — **no new always-on process model invented**, and the TUI's own loop is untouched:

```
agent-vesper-tui --headless daemon        # or: agent-vesper-daemon (alias binary in the same app)
```

The daemon is a **separate composition** over the same `vesper-harness` runtime the TUI uses: it constructs `HarnessRuntime::new_with_checkpoint_gate(stores, cron_root, …)` with `checkpoints_enabled = true`, spawns `run_cron_scheduler` on its **own runtime**, and adds a **watcher poller**. It shares nothing mutable with the TUI process: coordination is through the durable stores only (`cron.jsonl`, session store, cognition DBs), exactly like qm's blue-green instances coordinating through Postgres.

- **Single-writer discipline** (qm's leader lease, single-user adaptation): daemon startup takes an exclusive `flock` on `<state>/daemon.lock`. Second instance exits 0 with "daemon already running (pid …)". The lock file carries the pid and start time; stale-lock detection is pid-liveness only (no lease renewal protocol needed single-user). The TUI never takes this lock; it keeps its in-process scheduler for foreground `/loop` fires while a daemon is absent — dual-fire is prevented by the slot-claim rule below.
- **Exactly-once fire** (port of qm's `claimSlot`/`markFired`): `CronRegistry` gains `claim_slot(job_id, slot) -> bool` and `mark_fired`, SQLite/JSONL-atomic, so the TUI scheduler and a daemon can coexist without double-firing the same slot. This is the one durable change to `vesper-checkpoints`, additive and default-safe.
- **Watcher poller** (new, modeled on qm's `monitor-poller`): file/process watchers registered from the TUI or daemon via a small `watch` tool/`/watch` command, stored in `watchers.jsonl`. The daemon polls on a 10s sweep (`createSweeper` cadence): fs events via `notify` crate (already implied by worker/checkpoint IO — verify at implementation; otherwise a bounded directory mtime scan), process liveness via the existing process registry. Each watcher carries: scope id, target (path/glob or process id), a **literal** line pattern with optional `^`/`$` anchors (qm's `compileMonitorPattern` restriction — no regex metacharacters, anti-ReDoS and anti-aliasing), optional quiet-heartbeat interval, and a fire prompt.
- **Fan-out cap and rate limit** (qm constants, adapted): ≤ 20 watcher fires per sweep, ≥ 60s between fires of the same watcher, 180s default heartbeat. Over-cap events queue to the next sweep, never dropped silently.
- **Fires are bounded turns**, not interactive sessions: each fire runs a provider turn with the same permission floor as an interactive turn — **default `Ask` mode**; in headless daemon context with no approval channel, `Ask` fails closed via `DenyPermissionPort` (existing `vesper-agent/src/permission.rs`), so unattended fires are read-mostly unless the user has explicitly configured `AGENT_VESPER_FIREWALL`/bypass settings permitting more. That is the deliberate safety shape: cron work gets no free authority the interactive session has not granted.
- **No blocking of the TUI**: daemon mode is a separate process, so the interactive TUI's render loop is untouched by construction. In-process, the watcher sweep runs on the runtime the harness already owns (it already spawns `run_cron_scheduler`), never on the render task; verified by the input-latency gate (§5.5).
- **TUI surfacing** (host parity): `/daemon status` shows lock-holder pid, next scheduled fire, active watchers; a fired cron's result lands in the session transcript and, when the TUI is open, as a turn entry. ACP host surfaces the same via session control state (`vesper-acp/src/controls.rs`), no new protocol surface.

### 4.3 What we are NOT building

No Slack delivery, no run-activity SSE, no leader-election over multiple hosts, no `wake` envelope routing, no idempotency store beyond slot claims, no ambient judge. The daemon's output surface is the local transcript + TUI status, because that is the whole surface a single-user tool has.

### 4.4 Acceptance criteria

1. Two daemons: second exits cleanly with a clear message; stale lock (dead pid) is reclaimed.
2. Slot claim: TUI scheduler + daemon pointed at the same `cron.jsonl` fire each slot exactly once across 100 simulated slots (deterministic test with injected clock).
3. Watcher literal-pattern validation rejects `a.*b` (regex metachar), accepts `error:` / `^FAIL` / `done$` alternatives; fires carry the matched tail bounded to 4 KiB (qm's `MAX_TAIL_CHARS` discipline).
4. Rate limit: 61s-since-last-fire watcher with new output does not fire until the window passes (injected clock).
5. Headless `Ask` fails closed: a cron fire requiring approval is denied, logged as such, and does not retry indefinitely (bounded retries: 3, then pause job + surface in `/daemon status`).
6. TUI input-latency regression gate green (§5.5): with daemon-mode watchers active in-process, p99 keystroke-to-render stays within the gate.
7. No new always-on file descriptors: daemon holds only the lock file + stores; a test enumerates open fds before/after provisioning a watcher and asserts no growth attributable to sweep bookkeeping.

---

## 5. Verification & Phasing

### 5.1 Sequencing rationale

F1 first because it is pure, tiny, and closes the worst hole (Bypass + `rm -rf /`) with no new infrastructure. F2 second because its capability contract already exists and F1's residual risks (§1.5) are only contained by a boundary. F3 third because F2's scope-demanded isolation needs a scope object to hang from, and because memory scoping already exists (ADR-0021) so F3 is mostly binding, not building. F4 last because it consumes all three: cron fires are subject to the firewall, may demand sandboxing per scope, and write to scope-keyed stores.

### 5.2 Phased PRs (strictly isolated, one capability per PR)

**PR-1 — `vesper-policy` firewall core (pure). — LANDED (35 policy tests green; workspace floor 1,253)**
Scope: `crates/vesper-policy/src/firewall/` (mod/normalize/rules) + tests + Cargo `regex` unicode-perl override (mirrors `vesper-agent`/`vesper-cognition` local overrides; does not leak). No wiring into any executor; the module is inert until PR-2. Scan matches rules against normalized, lowercased text only — never the raw command — so heredoc stripping and segmentation are security-meaningful. **Rule-authoring constraint (root-caused this PR): never `to_lowercase()` compiled patterns — it corrupts `\S`/`\W`/`\D` classes; patterns are authored lowercase and scan text is lowercased instead. No lookaround (`regex` does not support it).** Exit: all §1.6 acceptance criteria; floor now 1,253 (was 1,225; +35 policy tests, 18 pre-existing).
Risk: none to runtime (inert module).

**PR-2 — Executor wiring + both hosts.**
Scope: `vesper-runtime` `registry.rs` scan call in `RunCommand::execute`; shared instance compiled at host boot; TUI `/firewall` view + `/status` line; ACP gateway same instance; cross-host shared-instance test. Default **on** for the deny rules, off for `require_approval` rules (they only bind in Ask mode anyway). Exit: §1.6 all green in both hosts; bypass fast-path test; latency gate unchanged.
Risk: touches the hot path — mitigated by the fast-path budget test and by keeping scan outside `spawn_blocking`'s command clone.
Rollback: env `AGENT_VESPER_FIREWALL=off` returns the executor to byte-identical behavior; one test asserts the off path is structurally the old path (single flag check).

**PR-3 — `vesper-sandbox` crate, namespaces backend.**
Scope: new crate, Linux namespaces backend, capability probing, `IsolationRequirement` demand plumbing in `PolicyInput` (already present) plus registry routing. Opt-in only; nothing default-demands isolation. Exit: §2.5 criteria 1–3; Linux CI green; non-Linux builds compile with honest `Unavailable` caps.
Risk: new syscalls on Linux CI; contained by `#[cfg]` and fixture-gated tests.

**PR-4 — Docker backend + scope demands.**
Scope: feature-gated `docker` backend; `.agent-vesper/config.toml` `[sandbox]` demand parsing into `WorkspaceScope.sandbox`; `/sandbox` TUI command + ACP control mirror. Exit: §2.5 all; `#[ignore]`-gated docker tests documented for local runs.
Risk: Docker absence on CI — tests must assert honest refusal, never a skip-as-pass.

**PR-5 — `WorkspaceScope` resolution + skills/memory binding.**
Scope: `crates/vesper-agent/src/vro/scope.rs` pure module + host boot wiring in TUI and ACP; `AGENT_VESPER_EXTRA_SCOPES`; per-scope skills loading with safe paths; firewall config layering (consumes PR-1 composition). Exit: §3.4 all; both hosts resolve identical scope ids; ADR-0021 tests untouched.
Risk: skill-loading changes affect advertised tools — mitigated by per-scope rebuild at boot only (never mid-session), preserving loop-layer invariance.

**PR-6 — Cron slot claims + daemon lock.**
Scope: `vesper-checkpoints` `claim_slot`/`mark_fired` (additive, SQLite/JSONL-atomic); `daemon.lock` flock discipline; `--headless daemon` composition in the TUI app using the existing harness runtime; `/daemon status`. Exit: §4.4 criteria 1–2, 5; no change to foreground `/loop` behavior; TUI interactive tests untouched.
Risk: durability semantics — mitigated by injected-clock deterministic tests and by keeping foreground scheduling behavior byte-identical when no daemon exists.

**PR-7 — Watcher poller + TUI/ACP surfacing.**
Scope: `watchers.jsonl` store, literal-pattern validation, daemon sweep loop, `/watch` command, ACP control mirror, bounded retries/pause. Exit: §4.4 criteria 3–4, 6–7.
Risk: in-process sweep must never touch the render path — mitigated by the latency gate and by running the sweep on the harness-owned runtime task.

**PR-8 — Cross-feature integration + docs.**
Scope: end-to-end fixture (cron fire → firewall → optional sandbox → scope-keyed transcript), `docs/migration-status.md` entry, nearest `AGENTS.md` updates, evidence index. Exit: DOX closeout per root contract; full CI green; floors ≥ predecessors on every touched crate.

### 5.3 Merge gates per PR (all of these, every time)

1. `cargo test --workspace` — full floor, no deletions.
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
3. `cargo xtask architecture` — dependency direction and crate boundaries.
4. `cargo xtask msrv` — MSRV 1.88 (Project Contract).
5. Host-parity check: any host-agnostic capability added in one host is wired in the other in the same PR (Project Contract; verified by the cross-host tests named above).
6. No live provider calls or user-state writes in foundation verification (Project Contract).
7. DOX pass: nearest `AGENTS.md` updated where behavior/contracts changed.

### 5.4 Test-floor accounting

The 1,236 floor is a floor, not a target. Each PR's description must state: tests added (by crate), tests modified (must be zero unless the PR explicitly re-baselines with justification), and the resulting floor. PR-2 explicitly asserts the `AGENT_VESPER_FIREWALL=off` path is the unchanged legacy path; PR-5 explicitly asserts default scope behavior is byte-identical when no extra scopes are configured. Any PR that cannot show monotonic non-decreasing floor counts does not merge.

### 5.5 TUI responsiveness gate (new, ships with PR-7)

A `criterion`-free bounded benchmark: 10,000 synthetic keystroke events driven through the real dispatch path with watchers active, measuring p50/p99 dispatch-to-render latency on a headless terminal buffer. Gate: p99 must not regress by more than 5% against a recorded baseline committed alongside the test (baseline recorded on the PR-6 merge, pre-watchers). If CI variance makes a hard gate flaky, the test degrades to asserting the *structural* property instead (no `.await` on the render task that can be outrun by sweep bookkeeping; sweep never holds a lock the dispatcher needs) — but the structural assertion is mandatory either way.

### 5.6 Security review questions every PR must answer

Ported from qm's SECURITY.md discipline, adapted to single-user:

1. Does any change let a denied operation re-enter through a wrapper (nested workflow laundering)? (`PolicyEvaluator::evaluate_workflow` must stay authoritative.)
2. Does any change provision credentials into a sandbox or watcher context?
3. Does any change let a project scope weaken a global deny?
4. Does any unattended fire gain authority the interactive session has not granted?
5. Is every "unavailable" capability reported honestly rather than assumed?

---

## 6. Open questions (resolve before PR-1, none blocking the PRD)

1. **`notify` crate for fs watchers**: confirm whether an existing workspace dependency covers directory watching or whether PR-7 adds it (supply-chain gate: advisory, source, wildcard-dependency checks remain fail-closed).
2. **Firewall default-on vs default-off at PR-2**: this PRD proposes default-on for deny rules. If Alex prefers opt-in-everything, PR-2 ships default-off and PR-8 flips it after a soak period. Decision recorded in ADR when accepted.
3. **`ScopeId` stability across renames**: canonical-path-hash means renaming a project directory re-keys its stores. Acceptable single-user? (qm has the same property via channel ids.) Mitigation if not: a `scope-id` stamp file written on first boot.
4. **Docker feature gate vs always-compiled**: always-compiled with runtime probing would let `/sandbox status` report honestly on machines without Docker; feature-gated keeps the default binary lean. Lean wins unless probing is free.
