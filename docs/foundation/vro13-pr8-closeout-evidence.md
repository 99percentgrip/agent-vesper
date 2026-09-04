# VRO-13 PR-8 Closeout Evidence

Status: COMPLETE (PR-1 through PR-8 landed)

Owner: `docs/qm-extraction-prd.md` (the PRD) · this file records the PR-8
cross-feature integration and documentation-closeout evidence per the
PRD §5.2 PR-8 exit criteria.

## End-to-end fixture

`crates/vesper-harness/tests/vro13_e2e.rs` composes the full PR-8 pipeline
at the layer that owns it (`vesper-harness`, the composition adapter both
hosts share):

cron/watcher trigger → bounded background fire turn → shell command →
composed `CommandFirewall` (deny-precedence) → opt-in sandbox route
(fail-closed) → scope-keyed transcript.

| # | Test | Pipeline stages exercised |
|---|---|---|
| 1 | `pipeline_denies_destructive_command_from_a_watcher_fire` | Watcher (`watchers.jsonl`) fires via real `run_sweep_once` → exactly-once `claim_slot` → bounded `AgentLoop` turn (scripted provider, no live calls) attempts `rm -rf /` under Bypass → composed firewall denies with the exact `[VRO-13 Firewall] denied:` observation → outcome recorded via `mark_fired` into `cron-slots.jsonl` under the scope's state root |
| 2 | `pipeline_sandboxes_allowed_commands_and_logs_scope_keyed_transcript` | Same trigger path with the fixture's active `[sandbox] filesystem = true` demand (parsed by the real `vesper-config` reader) → benign command passes the firewall → fail-closed `satisfies_demand` gate → executes exactly once through the sandbox backend port → outcome in `cron-slots.jsonl` + `watcher-events.jsonl` (scope-keyed); a 30 s re-sweep within the rate window queues instead of double-firing |
| 3 | `pipeline_enforces_composed_project_rules` | PR-5 layer composition: a project rule (`deploy-prod`) composed onto the base ruleset denies its command class through the same turn path |
| 4 | `unattended_ask_fire_fails_closed_before_execution` | The PRD §4.2 safety shape: an unattended `Ask` fire with no approval channel (default `DenyPermissionPort`) is denied at the permission gate before any firewall scan or execution; the sandbox port is never reached |
| 5 | `unsatisfiable_docker_demand_refuses_instead_of_running_unsandboxed` | PR-4 fail-closed routing: an all-`Unavailable` backend (what a feature-off build resolves for a Docker demand) yields the model-facing `sandbox unavailable … refusing to run unsandboxed` refusal, never a silent unsandboxed run |
| 6 | `docker_feature_cold_start_guard_refuses_through_the_composed_path` (`--features docker` only) | The real `DockerBackend`'s cold-start guard (daemon probe against a binary that cannot exist) refuses before any `docker run`, surfacing through the composed executor path |

Determinism: the provider is scripted (`vesper-testkit::FakeProviderSession`
registered behind a real `ProviderRegistry`), every sweep/scheduler clock
input is injected, and no test requires a Docker daemon (the real-daemon
layer stays behind the same `#[ignore]` + `DOCKER_AVAILABLE` gate as
`crates/vesper-sandbox/tests/docker.rs`).

## Documentation closeout

- `docs/migration-status.md` — VRO-13 completed row with per-feature
  evidence pointers.
- `crates/vesper-harness/AGENTS.md` — e2e fixture + test-only dev-dependency
  note (`vesper-policy`, `vesper-provider`, `vesper-testkit`) recorded under
  Verification.
- `docs/qm-extraction-prd.md` — status header updated to PR-1..PR-8 landed.

## Test-floor accounting (PRD §5.4)

- Tests added: 5 (default features) / 6 (`--features docker`), all in
  `vesper-harness`.
- Tests modified or deleted: 0.
- Resulting floor: 1,415 → 1,420 (default) — monotonic, non-decreasing.

## Verification (merge gates, PRD §5.3)

Run on the PR-8 change set:

1. `cargo test --workspace` — full floor green.
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
3. `cargo xtask architecture` — dependency direction and crate boundaries
   green for all 23 packages. The scanner's allowlist binds every normal
   dependency unchanged; PR-8 adds one narrowly scoped dev-kind allowance
   (`vesper-harness` tests may link `vesper-policy`, `vesper-provider`,
   `vesper-testkit`) so the cross-feature fixture can compose the real
   seams without extending any production dependency edge.
4. `cargo xtask msrv` — not re-run for PR-8: the 1.88.0 toolchain is not
   installed on this host. No MSRV-sensitive construct was introduced
   (edition 2024 syntax already pinned by the workspace; the fixture and
   xtask change use only constructs the existing crates already compile).

## Security review answers (PRD §5.6)

1. No change lets a denied operation re-enter through a wrapper — the
   fixture asserts deny verdicts flow through the same `tool error:`
   observation channel the loop already classifies as failure.
2. No credentials are provisioned into any sandbox or watcher context —
   the fixture's sandbox backend is a deterministic recording stub, and
   `SandboxSpec`'s env allowlist is unchanged.
3. No change lets a project scope weaken a global deny — test 3 pins
   deny-precedence composition (project rules can only tighten).
4. No unattended fire gains authority: test 4 pins the fail-closed
   permission gate ahead of the firewall and executor.
5. Every "unavailable" capability is reported honestly — tests 5/6 pin the
   all-`Unavailable` and cold-start refusal shapes through the composed
   path.
