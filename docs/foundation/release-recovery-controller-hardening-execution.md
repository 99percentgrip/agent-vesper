# Release Recovery Controller — deterministic-policy hardening execution

**Date:** 2026-09-29
**Status:** PARTIAL IMPLEMENTATION; POLICY CORE AND SHARED HOST ROUTES GREEN; END-TO-END RELEASE EXECUTOR STILL OPEN
**Owning PRD:** [`../Agent_Vesper_Release_Recovery_Controller_PRD.md`](../Agent_Vesper_Release_Recovery_Controller_PRD.md)

## Objective

Continue the RRC implementation beyond its initial PR-1 through bounded PR-3
foundation without weakening the P0 PRD: harden exact-gate accounting,
deterministic controller directives/events, focused-repair and retry admission,
restart/resume, last-green comparison, immutable publication boundaries,
GitHub input handling, and shared TUI/ACP command semantics.

This report does **not** claim the complete PRD. The native background executor
that runs local gates, binds model repairs to independently observed tool
receipts, promotes a recovery worktree, creates/publishes a release under the
existing permission firewall, checks official GitHub status, and completes the
controlled real-GitHub red/green acceptance is not present. Those are release
gates in PRD sections 31–32 and remain open.

## Methods and commands

Source and contract inspection included:

```text
AGENTS.md
crates/AGENTS.md
crates/vesper-domain/AGENTS.md
crates/vesper-harness/AGENTS.md
apps/AGENTS.md
apps/agent-vesper-tui/AGENTS.md
apps/agent-vesper-acp/AGENTS.md
docs/AGENTS.md
docs/foundation/AGENTS.md
docs/Agent_Vesper_Release_Recovery_Controller_PRD.md
crates/vesper-harness/src/release_recovery.rs
crates/vesper-harness/src/host_commands.rs
apps/agent-vesper-tui/src/{commands,main}.rs
apps/agent-vesper-acp/src/lib.rs
.github/workflows/{ci,msrv,platform-foundation,web-driver,release}.yml
```

Verification and read-only live evidence commands:

```text
cargo test -p vesper-harness release_recovery --lib
cargo test -p vesper-domain -p vesper-harness -p agent-vesper-tui -p agent-vesper-acp --all-features
cargo test -p vesper-harness --all-features
cargo check --workspace --all-targets --all-features
cargo clippy -p vesper-domain -p vesper-harness -p agent-vesper-tui -p agent-vesper-acp --all-targets --all-features -- -D warnings
cargo xtask architecture
cargo xtask acceptance
cargo fmt --all -- --check
gh auth status
gh run list --commit 0b5630d271965e7d9df3a0a09c116c7f8c44042e --limit 20 --json databaseId,workflowName,status,conclusion,headSha,url
gh api repos/99percentgrip/agent-vesper/actions/runs/36534993345/jobs?filter=all\&per_page=100 --jq '{count: .total_count, sample: (.jobs[0] | {id,run_attempt,status,conclusion,name,labels,steps:(.steps|length)})}'
```

No provider call, Actions rerun, source push, tag, release publication, installer,
or replacement of the user's native installation occurred.

## Files

Production and contracts changed by the RRC work unit:

- `crates/vesper-harness/src/release_recovery.rs`
- `crates/vesper-harness/src/lib.rs`
- `crates/vesper-harness/src/host_commands.rs`
- `crates/vesper-harness/Cargo.toml`
- `crates/vesper-domain/src/slash_commands.rs`
- `apps/agent-vesper-tui/src/commands.rs`
- `apps/agent-vesper-tui/src/main.rs`
- `apps/agent-vesper-acp/src/lib.rs`
- `Cargo.lock`
- applicable `AGENTS.md` files
- the owning PRD, evidence index, initial execution report, and this report

Pre-existing unrelated modified and untracked workspace content was not removed,
rewritten, or claimed.

## Implemented behavior

- Added a typed `ReleaseDirective` selector and `ReleaseControllerEvent` reducer;
  adapters execute one controller-authorized operation and cannot infer lifecycle
  progression from model prose.
- Persisted exact candidate/ref, required gate names, run/attempt/job identities,
  transitions, failures, repair attempts, state changes, budgets, outage evidence,
  and publication/main-health separation.
- Pinned pre-release completeness to the four repository workflow names:
  `pull-request-validation`, `msrv`, `five-target-foundation`, and `web-driver`.
  Missing gates no longer look like a complete matrix, and only the newest run
  per workflow is retained from GitHub's descending run list.
- Counted one unchanged matrix refresh as one stagnant action instead of one
  action per gate; the watchdog escalates after the bounded action/time ceiling.
- Made failure capture idempotent by run/attempt/job/fingerprint and distinguished
  repeated fingerprints across attempts from duplicate jobs in one attempt.
- Required controller state `RetryAdmissible`, a matching repair fingerprint,
  changed commit, hypothesis, evidence, and passed focused proof before the one
  full-gate retry. Targeted diagnostic retries now have an explicit two-attempt
  budget. Two failed repairs in one causal family escalate.
- Added exact last-green comparison and immutable one-time source-touch context.
- Ensured a verified repair advances the candidate SHA, then clears stale matrix
  rows only when the admitted retry is dispatched.
- Added the demonstrated `release green -> publish -> docs closeout -> main red`
  regression: the release remains `PUBLISHED / VERIFIED`; tag/publication cannot
  be re-entered and artifacts report no evidence of impact.
- Added persisted external pause/reopen behavior and read-only `/release resume`
  refresh for GitHub-backed remote/waiting states. Resume derives and validates a
  safe `owner/repository` identity from the workspace remote and refreshes the
  exact SHA before progression.
- Hardened GitHub input handling: exact 40-character SHA requirement, safe repo
  identity, required nonzero API identities, checked attempt conversion, bounded
  untrusted names/URLs, redacted stderr, and no token argument.
- Hardened persistence with a 4 MiB load ceiling, lock-protected temporary-file
  persistence, file and best-effort directory synchronization, schema checking,
  and user-state roots outside arbitrary workspaces.
- Kept `/release` in the shared host executor in both hosts. ACP documentation and
  comments no longer describe it as a model workflow; `/ci` includes the same RRC
  projection; the command catalog advertises the actual controller actions.

## Exact evidence

### Focused RRC suite

```text
running 23 tests
...
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 127 filtered out
```

The cases directly cover partial/missing matrices, causal extraction,
normalization, secret canaries, stale SHA refusal, typed polling/backoff,
last-green context, retry/focused-proof admission, changed and repeated
fingerprints, repair-family exhaustion, official/community outage rules,
pause/restart identity, persistence of job IDs, the stagnation watchdog, and
published-release versus degraded-main separation.

### Complete affected harness suite

```text
running 203 tests
...
test result: ok. 198 passed; 0 failed; 6 ignored
```

The six ignored tests are explicit environment/live gates (workspace-layout,
MPRIS/Resolve, contained browser runtime, shared native sandbox, and real browser
worker acceptance). They were not represented as passing RRC evidence.

### Affected host/domain suites

The combined affected-package command completed successfully. Receipts include:

```text
agent-vesper-acp library: 61 passed; 0 failed
agent-vesper-tui library: 299 passed; 0 failed
agent-vesper-tui binary: 169 passed; 0 failed; 1 ignored
vesper-domain: 59 passed; 0 failed
```

The ignored TUI case requires a real container runtime and bundled Landlock image.

### Architecture, acceptance enforcement, lint, format, compilation

```text
architecture boundaries validated for 31 packages
Acceptance regression gate: 23 exact cases passed in 6659 ms.
Offline fixture model cost: zero; live-model effectiveness is not measured.
cargo check --workspace --all-targets --all-features: exit 0
cargo clippy ... -- -D warnings: exit 0
cargo fmt --all -- --check: exit 0
```

### Read-only live GitHub shape check

For exact commit `0b5630d271965e7d9df3a0a09c116c7f8c44042e`, `gh run list`
returned exact-SHA records for all four required workflow names. Three were
`success`; `five-target-foundation` was `cancelled`. This was observed, not
relabelled green. The jobs endpoint for canonical run `36534993345` returned:

```json
{"count":2,"sample":{"conclusion":"success","id":109296887485,"labels":["ubuntu-24.04"],"name":"supply-chain","run_attempt":1,"status":"completed","steps":8}}
```

This validates the production adapter's structured field assumptions. It is not
the controlled red/green GitHub acceptance required by PRD section 32.

## Acceptance trace

| Criterion | Current evidence | Verdict |
|---|---|---|
| AC-01 partial matrix blocks | focused test | Passed locally |
| AC-02 first causal error | panic extraction test | Passed locally |
| AC-03 identical/no-change block | cross-attempt fingerprint test | Passed locally |
| AC-04 repair hypothesis + proof | verified-repair admission test | Passed locally |
| AC-05 focused proof first | reducer and admission tests | Passed locally |
| AC-06 bounded retries | full/targeted/family budgets | Passed locally |
| AC-07 changed fingerprint | second-attempt diagnosis test | Passed locally |
| AC-08 last known green | exact comparison + immutability test | Passed locally |
| AC-09/11 community cannot classify outage | pure policy tests | Passed locally |
| AC-10 official + infra + repository evidence | pause test | Passed locally; official transport open |
| AC-12 mixed health unconfirmed | policy branch implemented | Covered by reducer policy; dedicated fixture still desirable |
| AC-13 pause/resume checkpoint | reopen/exact IDs/directive test | Passed locally; live resume not run |
| AC-14/15 published vs degraded main | incident-sequence test/status rendering | Passed locally |
| AC-16 provider neutrality | harness core has no provider branch; architecture gate | Passed structurally |
| AC-17 shared TUI/ACP state | shared executor/source and affected host suites | Passed structurally; dedicated two-process fixture open |
| AC-18 stale SHA | focused test | Passed locally |
| AC-19 secret canaries | redaction test and bounded persistence | Passed locally |
| AC-20 restart exact IDs | ledger reopen tests | Passed locally |
| AC-21 cancellation wording/state | shared command behavior | Implemented; dedicated process fixture open |
| AC-22 stagnation | six-action test | Passed locally |
| AC-23 demonstrated incident | green/publish/closeout-red regression | Passed locally |

## Deviations and unresolved items

1. **Full PRD completion is not claimed.** A controller-owned native executor is
   still required to run local verification, issue structured repair tasks,
   independently bind focused-proof receipts, promote a recovery worktree,
   dispatch permitted remote gates, tag, publish, verify assets, and perform
   post-release closeout without relying on a user/model to manually advance
   events.
2. Official `githubstatus.com` transport and its 10–15 second bounded request set
   are not implemented. The outage hierarchy/reducer exists and fails closed.
3. The current text status is shared by both hosts, but a dedicated asynchronous
   TUI panel/event stream and a dedicated ACP/TUI two-process parity fixture are
   still open.
4. Controlled real-GitHub deterministic red/green, rerun, publication-regression,
   and full cross-platform acceptance were not executed. The live command was
   read-only and the observed five-target run was cancelled, so it cannot satisfy
   the release gate.
5. No Actions write endpoint was invoked. This preserves permission safety, but
   means the retry-admission token has not yet been exercised against a controlled
   live repository.

## Readiness effect

The RRC policy core is materially stronger, deterministic, restart-safe, and
better aligned with AC-01 through AC-23. It prevents several concrete speculative
loop paths and preserves the published-release boundary. It is **not yet ready to
replace the complete release workflow autonomously** because the executor,
official-status transport, host-native asynchronous UX, and controlled live
acceptance remain release blockers.
