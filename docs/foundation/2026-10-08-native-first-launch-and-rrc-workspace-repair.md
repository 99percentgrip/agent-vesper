# Native first launch and RRC verification workspace repair

## Objective

Repair Alex's Windows first-launch report without platform/provider shortcuts;
require the same usable welcome/Settings flow on Linux, both macOS architectures
and Windows, and eliminate newly reproduced RRC verification-scope defects.

## Findings and changes

1. All four real adapters were already registered on every platform. Startup
   authenticated the default Z.ai adapter before presenting the welcome screen.
   Back propagated `authentication cancelled; a provider credential is required`
   and exited. The OpenAI background credential branch had the same modal failure.
   Existing terminal fixtures supplied a synthetic Z.ai key and bypassed this path.
2. Native startup now opens the welcome screen before authentication or account
   discovery. Explicit Settings and `/auth` retain adapter-owned authentication;
   missing credentials grant neither account/model access nor a provider request.
   Resume still bypasses the welcome screen. Authentication Back returns to Settings.
3. xAI's many advertised controls pushed Providers outside the initial viewport.
   Providers is now the first category for every adapter, without a model table or
   provider-name branch. Confirmed selection still uses the existing restart contract.
4. A cached `xtask` contained an old release worktree's `CARGO_MANIFEST_DIR`.
   An attempted current verification spawned Cargo in that old worktree and replaced
   the shared TUI artifact with the old executable. The wrong-scope check was stopped;
   it is explicitly not current-source proof. A red native experiment invoked the
   cached verifier from an empty private fixture corpus: it returned success and
   reported 76 scenarios from its old worktree.
5. `xtask` now resolves source AND fixture roots from the invoking directory,
   prints the actual workspace and refuses outside-workspace calls. The shared
   non-production testkit fixture resolver also selects the invoking workspace.
   Two native cached-binary regressions, the testkit invocation-root case and
   overflowing Settings-category case are mandatory acceptance. No shared cache, retry budget or journal was cleared.
6. Natural-language release admission was held behind pending OpenAI account
   discovery, although slash release controls were admitted. The native PTY case
   reproduced the blocked input. The shared conservative release-intent classifier
   now admits native release objectives without account discovery; normal prompts,
   negated/deferred release discussion and pasted content still await validation.
   A cross-platform pure case and Unix native case are mandatory acceptance.
7. Five-target prerequisites now build shipping-feature hosts and execute actual
   signed-out landing, four-provider authentication and shared TUI/ACP RRC controls.
   Windows uses an explicitly selected ConPTY with one hashed Python 3.13 wheel;
   POSIX uses stdlib PTYs. Canonical Windows also tests the real release-profile TUI,
   alongside the existing both-PowerShell installer and static-CRT package gates.

8. Production resource discovery was Linux-only: macOS/Windows refused local
   Cargo gates instead of operating with parity. Safe native observations now
   supply physical memory, swap/commit accounting and owned descendants, retaining
   Linux's ancestor-cgroup/PSI/zram path. A pinned Rust-1.88 `sysinfo` system-only
   dependency supplies native observations. Windows additionally samples inherited
   Job Object/per-process constraints through fixed read-only OS queries. Current
   private commitment is used; historical peaks cannot prevent resource recovery.
   Shared policy reserves, disk limits, leases, concurrency and retry budgets remain.
9. Worker registration no longer performs synchronous native discovery on the host
   admission path. Native probe waits inherit controller cancellation and refuse
   malformed/missing/inaccessible observations. A real constrained-Windows-job
   fixture and native observation/descendant cases are required hosted evidence.

10. The first exact v0.24.10 matrix reproduced two terminal-fixture faults after
    the signed-out welcome assertions passed: relative executable paths were
    interpreted in the child's private directory, and Windows CP1252 redirected
    output rejected the successful authentication receipt's Unicode arrow. The
    common host now resolves caller paths before changing cwd and explicitly
    writes UTF-8 receipts. ACP already exchanges UTF-8 JSON bytes; its executable
    path is resolved by the same caller-directory rule.
11. The shared RRC classified those typed Python exceptions as Unknown and safely
    stopped before repair. FileNotFoundError, UnicodeEncodeError and
    UnicodeDecodeError now provide strongly supported source-diagnosis evidence;
    permission/infrastructure priority and unknown-exception refusal remain.
    Two exact classifier cases are mandatory native acceptance. This is a
    diagnosis gap; the observed full-matrix wait is not a dead loop.

## Methods and commands

- Inspected both user screenshots, unconditional provider registration, startup,
  landing, Settings, auth cancellation, cache ownership and verifier root selection.
- `python3 apps/agent-vesper-tui/tests/first_launch_pty.py <native-binary>`:
  retained old-source red and current shipping-feature process receipts.
- `python3 apps/agent-vesper-tui/tests/settings_auth_pty.py <native-binary>`:
  focused xAI/OpenAI navigation, auth methods, secret masking and no-save Back.
- `python3 apps/agent-vesper-tui/tests/release_hosts_pty.py <tui> <acp>`:
  identical partial matrix/Published-main-degraded observation, no-write unsafe retry,
  cancellation retaining epoch/run/tag/assets and zero provider dispatch.
- `cargo test --locked -p xtask --test cached_workspace`: two native process cases
  pass for two invocation roots plus refusal outside a workspace.
- `cargo xtask verify`, `cargo xtask acceptance`, architecture, locked MSRV and native
  exact-commit prerequisites: current receipts are recorded below as they settle.
- All local expensive compilation uses the existing managed target with two build
  jobs and two test threads; native release gates remain governor-owned.

## Files and contracts

Source: TUI `main.rs`, `auth_hub.rs`, `lib.rs`; xtask `src/main.rs` and
`tests/cached_workspace.rs`; testkit `src/fixture.rs`; shared harness resource
observer, Windows query script, executor cancellation/admission, shared Python
runtime failure classification and real constrained
Job Object fixture, with manifest/lockfile and dependency-register changes. Process fixtures: `first_launch_pty.py`,
`settings_pty.py`, `settings_auth_pty.py` (unchanged test scenarios),
`release_hosts_pty.py`, `openai_startup_responsiveness.rs`,
`windows-terminal-requirements.txt`.
CI: canonical and five-target workflows. Installer completion guidance now leads
with welcome → Settings → Providers. Updated DOX: root, TUI, terminal tests,
xtask, testkit, scripts, workflows and foundation ownership. Updated user guides and owning Settings
and RRC PRDs, with evidence-index links in the same implementation candidate.
Parent apps/docs indexes retain their existing subtree boundaries and are unchanged.

## Evidence and acceptance

- Startup red: the published-source binary opened Z.ai Authentication and never
  reached `Start coding` with private missing credentials.
- Initial current shipping-feature Linux process run: all four signed-out provider
  cases passed landing, all four authentication panels, Back and clean Quit.
- Settings authentication passed locally. Both-host RRC process cases passed
  with all four adapters selected and signed out, without provider dispatch.
- Pending-account native release red reproduced input held behind discovery;
  corrected native suite: 4 passed, 0 failed. The first post-fix observation used
  a contiguous raw terminal phrase that ANSI cursor updates fragmented; the
  actual admission error was present. The observer now checks the causal
  `git-common-dir` refusal token, and preserves that failed observer receipt.
- Wrong-worktree cached verification: retained false-green red plus observed Cargo
  cwd pointing at the old exact v0.24.9 release worktree. This does not invalidate
  v0.24.9 checks of that same immutable source; it invalidates using that run as proof
  of these new changes. A later keyboard-fixture invocation reused the overwritten
  old TUI and is preserved as a failed artifact-identity attempt, not a new green.
- Native cached verifier tests: 2 passed, 0 failed. Current full verification must
  show `/tmp/vesper-rrc-closeout-fix` (or its exact native release worktree), never
  an embedded historical root.
- Initial repaired-source workspace verification passed formatting, strict all-target/all-feature
  Clippy, the complete workspace and fixture/runtime/architecture checks, and
  **147/147 exact acceptance cases**. The invocation log names this current worktree.
- Native resource cases passed locally: 30 selected cases, including real physical
  capacity, actual child/descendant measurement and cleanup, current Windows
  constraint math, malformed-accounting refusal and cancellation during a held
  observation. Windows/macOS production source also type-checks against all three
  target triples through an isolated harness that imports the actual files directly;
  this is compile proof only and does not substitute for native execution.
- Supply-chain advisory, ban, license and source gates passed; ten exact-release
  policy tests passed. Final shipping-feature process reruns passed: four clean
  first-launch cases, two Settings/authentication cases and four selected-provider
  TUI/ACP RRC cases. Exact five native interactive lanes, producing assets and
  native RRC Complete/Idle remain pending.

## Deviations and unresolved boundaries

The first correct-workspace broad run reached the later provider/testkit check and
failed on an undeclared `tempfile` used by the new fixture. The case now reuses
the existing RAII `LegacyStoreBuilder`, adding no dependency. That failed receipt
is retained; final-source verification is rerun after this correction.


Foundation verification makes no live provider/account/inference request and
uses no real credential stores. The separately authorized production release uses
native configured credential ports, without printing credential contents. No
installer replaces Alex's installation, and no manual version/tag/publication,
separate documentation-only push or disposable release driver is used. During
production release builds, three confirmed inactive historical Cargo target caches
were reclaimed with Cargo's scoped clean operation; source, active caches and
user state were preserved. The retained reclamation receipt records the targets
and actual removed counts. Test state is private; GLM's deliberately malformed vault prevents keyring fallback and other
providers use signed-out tombstones. Native Windows fixture transport comes from
[PyWinpty's own implementation](https://github.com/andfoy/pywinpty/blob/main/winpty/ptyprocess.py)
and [ConPTY backend declaration](https://github.com/andfoy/pywinpty/blob/main/winpty/enums.py).
Its CI-only 3.0.5 CPython-3.13 x86_64 wheel SHA-256 is pinned; production has no Python
terminal dependency. Hosted Windows 2025 is native execution, not an observation of
Alex's original Windows 10 laptop. Device/account and optional hardware capabilities
cannot be inferred from offline welcome/Settings/RRC checks. No universal absence
of future bugs is claimed.

Native resource sources: [sysinfo 0.37.2](https://docs.rs/sysinfo/0.37.2/sysinfo/)
(Rust 1.88, MIT), [Microsoft job queries](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-queryinformationjobobject),
[job memory limits](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information),
and [process commitment accounting](https://learn.microsoft.com/en-us/windows/win32/psapi/process-memory-usage-information).
Linux-only PSI/zram values remain explicitly unavailable on other platforms;
Windows swap fields describe the native commit extension above physical RAM.
Job observation applies to the inherited immediate Job Object; it does not invent
undiscoverable ancestor handles or claim arbitrary nested-job constraints.

## v0.24.10 first exact-candidate CI and follow-up

Candidate `960fe620d5f5a2f9071b76379bf64b9644fccd77` was prepared and pushed by
native RRC after all eight local gates passed, with its persisted retry budget
unchanged. Canonical `37743177494`, MSRV `37743177212` and web-driver
`37743177514` passed. Canonical Windows ran the real release-profile welcome
checks and actual private installs under PowerShell 5.1 and 7 successfully.
Five-target `37743177347` exposed the relative-path/receipt faults described
above. Passing foundational and welcome cases do not certify the skipped RRC
process or constrained-Windows-job checks. No v0.24.10 tag/publication is
permitted by this failed matrix.

Local red receipts reproduce both faults; the corrected authentication fixture
passes with `bin/tui` resolved from a private caller directory and
`PYTHONIOENCODING=cp1252`. The classifier red observed `(Unknown, Unknown)`;
its two corrected cases pass. Full current-source verification now passed formatting, strict Clippy, workspace
and process/fixture checks, architecture, and 149/149 exact acceptance. Fresh
shipping TUI/ACP process checks passed all ten cases with relative executable
arguments and CP1252 redirected output. The binding trace observes all 100
distinct mapped cases plus eight additional recovery cases (108 total); an
initial observer conflated those counts, then corrected its assertion without
altering any requirement or test. A fresh exact-SHA native matrix must pass
before publication under this same version. Native admission retains historical matrix and retry state.

## Readiness effect

Confirmed source defects are repaired; current-source and exact native-platform
acceptance is required before release/completion. The prior v0.24.9 publication
alone did not cover signed-out first-launch behavior. RRC owns the subsequent single
version bump, exact-SHA matrices, immutable tag/publication and registry/report
closeout; the invoking native owner must remain attached until Complete/Idle.

## Retained source-candidate receipts

[Evidence manifest](2026-10-08-native-first-launch-and-rrc-workspace-repair-evidence.json)
and [receipt archive](2026-10-08-native-first-launch-and-rrc-workspace-repair-receipts.tar.gz)
bind current source bytes, preserved failed attempts, invocation-scoped green gates
and frozen shipping-feature binary identities. Initial candidate results are retained separately from the corrected-source
[CI follow-up receipts](2026-10-08-native-first-launch-and-rrc-ci-follow-up-receipts.tar.gz).
All twelve initial prerequisite jobs settled; twenty native welcome cases and
Windows release-profile/private-install checks passed, while five downstream
fixture failures blocked publication. RRC captured all five causes, settled at
NeedMoreEvidence with its budget unchanged, and its idle UI quit with exit 0.
The terminal receipt is a bounded retained tail, not a complete initial transcript.
Corrected exact-SHA native prerequisites, constrained Windows job, publication
and final delivery remain pending.
