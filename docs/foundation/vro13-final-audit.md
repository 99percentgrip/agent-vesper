# VRO-13 Final Audit — All Phases (PR-1 through PR-8)

Status: **PASS** — all phases verified against the PRD
(`docs/qm-extraction-prd.md`), the frozen contracts, and live verification.
Audit date: PR-8 merge state. Auditor scope: implementation vs. PRD exit
criteria (§5.2), merge gates (§5.3), security questions (§5.6), and the
zero-degradation contract (§0.3).

Owner: `docs/qm-extraction-prd.md` · Implementation evidence:
`docs/foundation/vro13-pr8-closeout-evidence.md` (PR-8), per-phase entries in
`docs/migration-status.md`.

## Verification (live, this audit)

| Gate | Result |
|---|---|
| `cargo test --workspace` | **1,420 passed / 0 failed** (floor 1,236 → monotonically 1,282 → 1,322 → 1,415 → 1,420; never dropped) |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | **clean** |
| `cargo xtask architecture` | **23 packages validated** |
| `cargo xtask msrv` | **not run** — the 1.88.0 toolchain is not installed on this host; no MSRV-sensitive construct introduced (PR-8 only adds a test file and a scanner allowance; recorded honestly rather than claimed) |

## Phase-by-phase findings

### PR-1 — Firewall core (`vesper-policy::firewall`) — PASS

- Module shape matches the PRD §1.2 contract: `holder.rs` (process-global
  status + `AGENT_VESPER_FIREWALL` read-once), `normalize.rs` (the ported
  qm `scannableCommand` pipeline), `rules.rs` (default ruleset), compiled
  first-match semantics.
- Deny text is the stable contract shape: `ToolError::FirewallDenial`
  renders `[VRO-13 Firewall] denied: {reason}; matched: {rule}` and is
  classified as failure by VRO-12 (never success) — verified in
  `crates/vesper-agent/src/executor.rs:160-176`.
- Bypass composition (§1.3) is pinned three ways:
  `vesper-policy/src/lib.rs:289` (`fixture_bypass_plus_deny_is_absolute`),
  `vesper-policy/src/firewall/mod.rs:299` (`deny_still_denies_in_bypass_mode`),
  `vesper-agent/tests/firewall.rs:66` (`bypass_mode_still_honors_firewall_deny`).
- The off-path is structural: `ctx.firewall == None` when
  `AGENT_VESPER_FIREWALL=off` → no scan, no allocation (`tools.rs:424-451`).

### PR-2 — Executor wiring — PASS

- **One choke point**: `RunCommand::execute`
  (`crates/vesper-agent/src/tools.rs:424`) scans before `spawn_blocking`;
  `vro/react.rs:355` maps the denial into the model-facing observation. No
  TUI-side duplicate exists (grep across both hosts shows the executor as
  the only scan call site outside tests).
- `/permission` untouched; `/firewall` is view/disable-with-restart only;
  `/status` carries the read-only Firewall line.

### PR-3 — `vesper-sandbox` (ADR 0022) — PASS

- **100% safe library**: `lib.rs:1` `#![forbid(unsafe_code)]`; the only
  `unsafe` in the crate lives in `src/bin/sandbox_init.rs`, which is
  `#![allow(unsafe_code)]` + `deny(unsafe_op_in_unsafe_fn)` +
  `deny(clippy::undocumented_unsafe_blocks)` with `SAFETY:` comments on
  every block — exactly the ADR 0022 boundary.
- Raw syscalls (`unshare`, `fork`, `execve`, `prctl(PDEATHSIG)`, `waitpid`)
  are confined to the supervisor binary; the library spawns it through
  safe `std::process` code.
- No default-build dependency growth; `notify` crate deliberately absent
  (`watcher_sweep.rs:5`).

### PR-4 — Docker backend + scope demands — PASS

- Feature-gated `docker = []` (`vesper-sandbox/Cargo.toml`), zero new
  mandatory deps; both host apps forward the feature.
- Cold-start guard: `DockerBackend::probe_daemon` runs a bounded (5 s)
  `docker version` probe; every failure mode — binary missing, daemon
  unreachable, timeout, empty output — is an honest `Err`, never an
  assumed capability (`docker.rs:181-229`, pinned by
  `unreachable_daemon_reports_every_capability_unavailable`).
- Capability honesty pinned both layers: backend unit tests
  (`docker.rs:652-661`) and gated real-daemon integration tests
  (`tests/docker.rs`, `#[ignore]` + `DOCKER_AVAILABLE`, run with
  `-- --ignored`; never skip-as-pass: the two honest-refusal tests run
  ungated everywhere).
- Resource constraints honored: `--cpus`/`--memory`/`--pids-limit`,
  root bind-mounted at `/workspace`, `--network none` unless granted.
- Host parity: `/sandbox on|off|status` (plus `enable`/`disable` aliases)
  exists in both hosts with identical semantics and response text
  (TUI `commands.rs:955`, ACP `lib.rs:886`, ACP AGENTS.md §Sandbox);
  `on`/`off` are honest restart instructions, not fake runtime toggles.

### PR-5 — `WorkspaceScope` — PASS

- Stamp-pinned identity (§6.3 mitigation **adopted**): `.vesper-scope-id`
  stamp; absence → SHA-256-derived, stamped, reused — renaming a project
  directory does not re-key its stores (`scope.rs` module docs + STAMP
  constants).
- Layer discipline: L0 project RW / L1 global RO / L2 opt-in extra scopes
  (`AGENT_VESPER_EXTRA_SCOPES`, **dormant by default** — unset means
  byte-identical two-layer resolution).
- Firewall composition is deny-precedence only
  (`global ∪ project`, project may tighten, never un-deny) — same
  contract pinned by the PR-8 fixture's project-rule arm.
- ACP durable-state opt-in honored: the auto-spawned ACP host persists the
  stamp only under `AGENT_VESPER_ENABLE_SCOPE_STAMP=1` (root contract:
  no `.agent-vesper/` durable state in arbitrary project dirs by default).
- ADR-0021 cognition binding is one shared derivation for both hosts;
  routing semantics stay owned by the TUI, not duplicated.

### PR-6 — Cron slots + daemon lock — PASS

- Exactly-once slots: `claim_slot`/`mark_fired` additive on
  `CronRegistry`, kernel-arbitrated `O_CREAT|O_EXCL` marker under
  `slots/` (harness `lib.rs:2494`), outcome recorded scope-keyed.
- Single-writer `daemon.lock`: `create_new` discipline with recorded pid;
  stale/corrupt locks are honestly classified
  (`NotHeld`/`Held`/`Stale`/`Corrupt`) and reclaimable; `Drop` removes the
  file. TUI acquires once at daemon boot (`main.rs:198`), `/daemon status`
  reads the same honest classifier (`main.rs:12053+`).
- No change to foreground `/loop` behavior; interactive tests untouched.

### PR-7 — Watchers — PASS

- Polling sweep (no `notify` dependency — supply-chain decision recorded),
  literal patterns, `watchers.jsonl` store.
- Rate limits: re-fire window `max(60 s, heartbeat)`; **rate-suppressed
  matches queue, never drop** (`watcher_sweep.rs:148-163`, pinned by
  `rate_limit_requires_sixty_seconds_between_fires_of_one_watcher`).
- Bounded fan-out via capacity; over-capacity matches queue to the next
  sweep (`evaluate_sweep_pure_core_respects_cap_and_rates`).
- Render-path discipline: the sweep owns its own runtime task; no
  render-path `.await` (structural assertion, §5.5).

### PR-8 — Cross-feature fixture + closeout — PASS

- `crates/vesper-harness/tests/vro13_e2e.rs` (6 tests: 5 default, 6th
  under `--features docker`) composes the real seams: watcher fire via the
  real `run_sweep_once` → exactly-once `claim_slot` → bounded
  `AgentLoop` turn (scripted provider, no live calls) → composed firewall
  → fail-closed sandbox route → `mark_fired` scope-keyed transcript.
  Safety arms: destructive-deny under Bypass, unattended `Ask` fails
  closed at the permission gate *before* the firewall/executor, all-
  `Unavailable` refuses rather than running unsandboxed, docker cold-start
  refusal through the composed path.
- Docs: migration-status row, PRD status header, harness + foundation
  AGENTS.md, evidence file. Architecture scanner gained one narrowly
  kind-scoped allowance (harness dev-deps) — production allowlist
  unchanged, recorded in the evidence file.

## §5.6 security questions — verified answers

1. **Denied re-entry through wrappers?** No — the deny flows through the
   same `tool error:` observation channel VRO-12 classifies as failure;
   `PolicyEvaluator::evaluate_workflow` stays authoritative; PR-8's
   fixture asserts the deny observation end-to-end.
2. **Credentials into sandbox/watcher contexts?** No — the fixture's
   backend is a deterministic recording stub; `SandboxSpec`'s env
   allowlist unchanged; docker uses `--network none` by default.
3. **Project scope weakening a global deny?** No — composition is
   deny-precedence-only, pinned by the fixture's project-rule arm and the
   scope module's own tests.
4. **Unattended fire gaining authority?** No — unattended `Ask` fails
   closed at the permission gate before the firewall or executor is
   consulted (fixture arm 4); the daemon fires bounded turns with no
   approval channel.
5. **Every "unavailable" honest?** Yes — capability probes fail closed,
   all-`Unavailable` yields the model-facing refusal, docker cold-start
   refuses before any `docker run`; no skip-as-pass anywhere.

## Residuals (honest, non-blocking)

- `cargo xtask msrv` unverified on this host (toolchain absent). No
  MSRV-sensitive construct introduced; CI owns the 1.88 gate.
- The 3 docker real-daemon tests are `#[ignore]`-gated by design; this
  container has no daemon, so they were not executed here (the ungated
  honest-refusal tests did run and pass).
- Pre-existing uncommitted edits in `crates/vesper-cognition/src/store.rs`
  and `crates/vesper-mcp/src/plugins.rs` (both `chunks_exact → as_chunks`
  Rust-1.88 idiom modernizations) predate VRO-13's audit; they are
  behavior-preserving, covered by the green 1,420 run, and unrelated to
  the VRO-13 change set.
- Two dirty files aside, the working tree carries only the VRO-13 change
  set plus `.agent/` (session artifacts).

## Verdict

Every VRO-13 phase is implemented, contract-pinned by tests, and
verified green on the audited tree. The PRD's §0.3 zero-degradation
contract holds: the floor rose monotonically (1,236 → 1,420), Bypass
keeps its structural fast path, VRO-12 remains unconditionally active,
and both hosts stayed in parity on every new surface. No blocking
findings. VRO-13 is **complete**.
