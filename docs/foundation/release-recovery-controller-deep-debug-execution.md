# Release Recovery Controller deep debug execution

**Date:** 2026-10-05  
**Objective:** Audit the current RRC against its binding PRD, reproduce defects,
repair production paths in both hosts, and retain every missing acceptance gate.
**Verdict:** Repairs implemented; local and current five-target fixture acceptance passed. Full PRD parity is not yet certified.

## Scope and method

- Read the root and owning DOX chains, the complete RRC PRD and historical receipts.
- Trace the actual background executor, GitHub adapter, native effects,
  persisted ledger, AgentLoop worktree repair and TUI/ACP factory wiring.
- Add regression assertions before repairing the initial policy defects.
  Initial core run: **9 failed, 26 passed**. The nine failures are retained in
  the receipt archive, rather than replaced with passing-only evidence.
- Check GitHub's primary [workflow-jobs](https://docs.github.com/en/rest/actions/workflow-jobs)
  and [workflow-runs](https://docs.github.com/en/rest/actions/workflow-runs)
  contracts; apply attempt-specific job queries and complete pagination.
- Execute local fixture-backed native orchestration, actual temporary Git
  worktrees, cancellation/restart subprocesses, exact acceptance, MSRV,
  workspace verification and workflow syntax checks.
- Assemble an isolated candidate from base HEAD
  `0b5630d271965e7d9df3a0a09c116c7f8c44042e` plus the current scoped RRC
  implementation. Primary workspace had substantial existing dirty/untracked
  implementation and unrelated assets; those were not reset or swept into a
  primary-repository commit.

## Defects and repairs

| Area | Finding | Repair and regression evidence |
|---|---|---|
| Workspace version | Root bump left member dependency pins stale, making the candidate unresolvable | Inventory all workspace manifests before mutation, update internal pins, admit all version paths; new native regression failed before fix and passes afterward |
| Workflow titles | REST run names changed with evaluated `run-name`, corrupting gate/fingerprint identity | Resolve canonical workflow-ID metadata; actual adapter probe and custom-title fixture |
| Windows Git fixture | Native patch fixture compared LF bytes after automatic CRLF checkout | Declare LF in the isolated fixture; first Windows failure retained; new matrix required |
| CI terminal controls | Current gh refuses ANSI-bearing raw logs; colors could interfere with credentials | Bounded explicit raw read on gh's control-sequence rejection, neutralize controls before redaction/causal parsing |
| CI command echoes | Runner-echoed shell script was mistaken for the actual panic | Skip Run metadata groups; real runtime signature fixture and actual log probe |
| Interleaved output | Authorization/exit wrappers reordered after the same panic and changed fingerprints | Fingerprint only causal diagnostics and assertion payload; retain complete bounded excerpt separately |
| Asset checksums | Metadata presence did not prove checksum content matched archive | Bounded checksum downloads must match both server asset digests and exact archive filename; read-only real v0.24.4 verification passed |
| Background lifecycle | Native worker stopped at a CI wait or classification boundary | Continuous bounded wait/refresh/repair progression; `background_controller_waits_then_publishes_without_continue_prompts` |
| Missing CI | No exact-SHA run could terminate progression as an adapter error | Missing gates remain pending; `missing_runs_are_pending_evidence_instead_of_an_executor_error` |
| Matrix identity | A new run at attempt 1 was rejected after an older run at attempt 3 | Compare attempt only within the same run ID; `a_new_workflow_run_can_start_at_attempt_one` |
| Matrix settlement | Visible terminal jobs could be mistaken for a completed workflow | Require terminal run and jobs; `a_running_workflow_with_only_terminal_visible_jobs_is_incomplete` |
| Required success | Skipped/neutral required jobs could authorize green | Every required outcome must succeed; `required_skipped_job_never_opens_tag_admission` |
| Adapter inventory | Job queries could mix attempts and ignore later pages | Exact-attempt endpoint, bounded full pagination, duplicate/missing metadata refusal; actual adapter fixtures |
| Workflow provenance | Same-SHA incidental workflow could substitute for a required gate | Validate required workflow path, event, branch and exact SHA |
| Reducer atomicity | A rejected repair could partially alter candidate state | Clone/validate/apply transaction; `rejected_repair_event_does_not_partially_mutate_the_record` |
| Source retry | Unchanged commit or terminal state could admit retry | Changed verified candidate and active state required; initial red-to-green cases |
| Rerun identity | GitHub rerun retains old SHA; direct write ports bypassed admission | Changed source uses exact candidate push; raw native reruns refuse; scoped run/job tokens |
| Infrastructure retry | Old attempt could satisfy a reserved rerun, or restart replay it | Persist budget and next-attempt floors before effect; stale attempts excluded |
| Permission | Native stages and repairs did not consistently inherit host policy | Both hosts inject mode/permission; stage approval plus native command firewall; denial fixtures |
| Cancellation | Stale writer could overwrite Cancelled; other host could keep running | Storage guard, shared cancellation watcher and local process-tree termination |
| Resume | Cancelled stage/counters/IDs could be lost | Explicit stage-preserving resume and fresh remote evidence before progression |
| Ownership | Two host processes could progress one repository | Lifetime owner lock plus atomic save lock; exclusive-owner fixture |
| Restart effects | Unsettled version/commit/push/tag/repair could be repeated | Persist nonrepeatable operation journal; uncertain restart stops for intervention |
| Epoch history | New release or later-main recovery could replace published history | Archive previous epoch; separate main recovery identity/counters; immutable publication |
| Outage evidence | Missing source-exclusion evidence could be treated as proof | Require explicitly absent source and last-green explanations, nonempty green local gates and official degradation |
| Outage recovery | Unknown/degraded official response could reopen pause | Only explicit healthy official recovery resumes; paused evidence remains retained |
| Source baseline | Production last-green comparison was not fully wired; root metadata and large path lists could be missed | Compare actual green gates and complete path inventory; build metadata differs unless only internal release versions change |
| Causal logs | Prefix truncation or cache warning could hide the actual compiler/panic | Extract cause before excerpt bounding; warning-wrapper filtering; large-prefix/cache-warning cases |
| Fingerprints | Context, line/column or process IDs changed equivalent failure identity | Normalize causal segment and volatile values; fingerprint fixtures |
| Secrets | URL userinfo/query values, Basic auth and quoted credential fields could persist | Recursive ledger-string redaction plus expanded credential patterns; canary fixtures |
| Local repair | Local gate error lost its causal record; dirty version seed blocked recovery | Preserve local failure; isolate seed, verify baseline and promote only repair delta |
| Patch composition | Newly added regression files could be omitted | Stage complete worker delta; actual Git fixture preserves seed and added test |
| Focused proof | Arbitrary successful shell/compile-only/list/zero-match could be accepted | Conservative command admission, native rerun after final edit, executed nonzero test result and identifiable failing Rust test |
| Subprocesses | CLI/network children could wait indefinitely or leak descendants | Bounded common runner, cancellation, output cap, process group/Job Object cleanup |
| Publication | Incomplete/empty assets, wrong workflow/tag SHA or version could count as Published | Exact settled tag workflow, annotated target, stable metadata, fourteen required nonempty uploaded archives/checksums and matching version |
| Published bytes | Release workflow used replacement uploads | Compare existing bytes and fail mismatch; upload only missing assets |
| Release notes | Fixed old-version narrative could ship with future versions | Current tag label and generated notes; preserve truthful local-speech/ACP limits |
| Completion records | Section 37 excluded a binding section 32 gate while claiming completion | Supersede completion claim; preserve historical receipts and current missing gates |

## Files

Production changes in this audit:

- `crates/vesper-harness/src/release_recovery.rs`, `release_executor.rs`,
  `lib.rs`, `Cargo.toml`; native repair integration fixtures in `tests/`.
- `apps/agent-vesper-acp/src/lib.rs` and `apps/agent-vesper-tui/src/main.rs`:
  invoking host mode/permission propagation.
- `xtask/src/main.rs`: explicit policy dependency boundary and expanded fixed
  acceptance cases. `.github/workflows/release.yml`: immutable upload and notes.
- `.github/workflows/release-recovery-lifecycle-acceptance.yml`: current
  controller/adapter/process and repair/retry checks on five native targets.
- Owning DOX docs, PRD section 37, evidence index, historical completion banner,
  this report and its source/receipt companions.

The domain command catalog, host command/parser changes, platform workflow,
root AGENTS, lockfile additions and earlier RRC reports already existed in the
initial dirty workspace. The source manifest binds the composed candidate,
without attributing all pre-existing changes to this audit.

## Exact verification

Full logs are retained in `release-recovery-controller-deep-debug-receipts.tar.gz`;
source hashes and receipt identities are in `release-recovery-controller-deep-debug-source.json`.

| Command / check | Exact result and scope |
|---|---|
| `cargo xtask verify` | Full repository checks passed; final acceptance printed **54 exact cases passed**. Includes workspace clippy, architecture, foundation/host/provider/session checks. This run predates the separate authentication UI repair below. |
| `cargo test -p vesper-harness --lib --tests --all-features --locked --offline` | **242 library passed, 6 ignored; 54 integration passed, 5 ignored**. Nested subprocess case prints a separate one-case receipt; it is not an extra independent test. |
| Same command with `cargo +1.88.0` | Same counts passed at the required MSRV. Ignored cases remain unexecuted. |
| Native GitHub run `37277822045` | Exact scoped RRC candidate **0f45e56329457b61f75399e00146670e1a13d3e5**, all five jobs succeeded: Linux x86_64/ARM64, macOS Intel/Apple Silicon, Windows x86_64. Contains current 68-case RRC suite and typed repair/retry acceptance. |
| Production-linked actual GitHub adapter probe | Passed canonical title, settled actual red/green jobs, causal fingerprint, canary redaction, baseline comparison and repeated-failure retry refusal. Controlled exact-SHA red/green/repeated/post-main runs are retained in JSON receipts. These Ubuntu-hosted synthetic platform jobs are not native platform proof. |
| Actual partial matrix probe | **757b9b127f2846b832334c5aebbd1e48f71cd4db**, run **37277823712**: observed failure with running/queued peers, retained WaitingForMatrix and blocked retry; classified only after fresh complete evidence. |
| Existing production publication read-only probe | v0.24.4 exact tag **d22113528362706fa1672051dcb9385c82c22d8d**, workflow **36515073932**, 16 assets and seven downloaded checksum contents verified. Historical publication verifier evidence; does not validate the modified producing workflow. |
| Auth repair follow-up | TUI authentication tests **12 passed**; full TUI library **300 passed**; native OpenAI adapter **48 passed**; final workspace clippy passed. `cargo build -p agent-vesper-tui --all-features --locked --offline` passed and produced `target/debug/agent-vesper-tui`; no installation was replaced. Initial `--bin` auth filter selected zero tests and is not acceptance evidence; correct `--lib` run is retained. |

The native candidate binds the scoped RRC sources; the subsequent authentication
UI repair has local evidence and is **not included in that five-target candidate**.
Final formatting and diff whitespace checks passed. Both edited workflows parse
as YAML and all 13 Bash bodies passed `bash -n`; delivery-document relative links
resolve after URL decoding. An initial link check omitted URL decoding and falsely
rejected an existing `%20` link; the corrected check passed.
No passing count substitutes for an unexecuted requirement.

## PRD acceptance traceability

“Local pass” means the named production-path fixture passed locally; it does
not certify real-provider, other-platform or production-publication behavior.

| Criterion | Current executable evidence | Remaining acceptance |
|---|---|---|
| AC-01 complete matrix | `partial_matrix_blocks_retry`, pending-run and terminal-run fixtures | Passed current controlled and actual partial-matrix probe |
| AC-02 causal evidence | `complete_matrix_collects_first_causal_log_and_classifies_it`, large-prefix, cache-warning, local-failure fixtures | Passed production-linked actual log probe |
| AC-03 unchanged fingerprint blocks retry | `identical_failure_on_new_attempt_without_change_hard_blocks_retry`, unchanged-commit fixture | Passed actual repeated-fingerprint probe |
| AC-04 hypothesis/proof | `verified_repair_is_the_only_path_to_one_full_gate_retry`, native focused-proof, coding-factory and typed repair integration fixtures | Real provider repair remains unexecuted |
| AC-05 platform focused proof | Native proof after edit, unit identity/nonzero enforcement, actual repair patch and coding-factory fixtures | Five native fixture targets passed; actual model/platform repair remains unexecuted |
| AC-06 bounded budget | `two_failed_focused_repairs_exhaust_the_causal_family_budget`, full/infrastructure attempt-floor cases | Passed native run 37277822045 |
| AC-07 changed failure diagnoses | `changed_fingerprint_after_retry_opens_a_new_diagnosis` | Current controlled scenario |
| AC-08 last-green comparison | `last_green_comparison_is_exact_and_immutable`; production baseline wiring | Actual controlled baseline probe passed; next-release baseline remains unexecuted |
| AC-09 deterministic error not outage | `deterministic_test_failure_cannot_be_relabelled_as_outage` | No community telemetry used |
| AC-10 confirmed outage pauses | `official_degradation_needs_repository_exclusion_and_infrastructure_evidence`, late-branch pause | No real outage induced |
| AC-11 community alone insufficient | `community_reports_alone_never_confirm_outage` | Local pass |
| AC-12 mixed health unconfirmed | Inconclusive source and deterministic mixed failure fixtures | Local pass |
| AC-13 pause identity/resume | `paused_epoch_reopens_with_exact_identity_and_resumes_through_remote_refresh` | No real outage induced |
| AC-14 immutable release | `green_release_then_red_closeout_keeps_publication_immutable`, archived-main-epoch case; no-clobber workflow | Current production publication unexecuted |
| AC-15 Published/main distinction | `status_projection_exposes_product_fields_without_causal_log_content`, separate-main fixture | Both host interactive UX observation unexecuted |
| AC-16 provider neutrality | `repair_factory_executes_real_tools_for_two_provider_fixtures`: two registered fixture factories drive actual file edit, command execution and native red-to-green Rust tests; no concrete provider import | Real provider transport/model judgment remains unexecuted; fixture-scope criterion passes |
| AC-17 host parity | Shared host operation, TUI routing acceptance, ACP/TUI mode wiring and workspace checks | Interactive both-host scenario unexecuted |
| AC-18 stale SHA | `stale_sha_cannot_advance_candidate`, provenance and attempt fixtures | Passed current exact-SHA adapter probe |
| AC-19 secrets | Ledger boundary, Basic/quoted/signed-URL and canary cases | Passed actual synthetic-canary log probe |
| AC-20 restart identity | `release_worker_cancel_restart_process_acceptance`, owner/archive fixtures | Passed native run 37277822045 |
| AC-21 cancellation | Actual local process-tree fixture, persisted cross-host cancellation case | Passed native run 37277822045 |
| AC-22 stagnation | `watchdog_triggers_after_six_stagnant_actions`; continuous passive CI waits do not spend action budget | Native active timeout path is bounded; no 20-minute hang induced |
| AC-23 release/docs/red/different-platform | `green_release_then_red_closeout_keeps_publication_immutable`, changed fingerprint and main epoch fixtures | Real published-current-release scenario unexecuted |

## Other binding requirements and section 32

| PRD scope | Evidence/implementation | Status boundary |
|---|---|---|
| Sections 1–7, 35–36: deterministic ownership/principles | Native controller owns effects; model is bounded repair worker | Local source/fixture evidence |
| Sections 8–11: typed state/failure/settlement | Transactional reducer, strict terminal workflow/jobs, unknown classifications stop | Local fixture evidence |
| Sections 12–13: structured API/polling/fingerprints | Exact-attempt paginated adapter, source provenance, bounded CLI and normalized causal evidence | Current remote/native acceptance tracked separately |
| Sections 14–16: budget/proof/private promotion | Reserved counters/floors, native focused proof, isolated Git seed/delta fixture, exact candidate push | Actual provider/platform repair limitations above |
| Sections 17–19: outage/pause/work ceiling | Official HTTPS request only, strict source exclusions, one-shot resume health, bounded wait and active watchdog | No real outage or prolonged active hang induced |
| Sections 20–22: publication/main and commands | Archived distinct epochs, shared commands, status projection and host factory wiring | Interactive UX and real publication remain open |
| Sections 23–24, 26–28: persistence/evidence/security/concurrency/cancel | String redaction, atomic bounded ledger, owner locks, immutable context, journals and cancellation watcher | Native fixture receipts; platform receipts tracked separately |
| Section 25: research order | Repair prompt supplies causal evidence and directs source/primary-document investigation | Actual model compliance not measured by fake provider |
| Section 30: test strategy | Core/adapter/parser/native/repair tests plus controlled workflow scenarios | Rate-limit behavior and real outage behavior are not live-certified |
| Section 31: implementation plan | PR-1–PR-7 foundations retained and repaired | Milestone labels do not grant completion |
| Section 32: RRC release gate | Local state/budget/parity/restart/outage gates; current remote receipts below | Current production release workflow/tag/assets still unexecuted |
| Sections 33–34: metrics/source basis | Persisted transition/counter/evidence data and primary GitHub sources | No live-model cost/effectiveness claim |

## Deviations and unresolved items

- One final workspace run reported all ten `r3_voice_pack` assertions passed,
  then its process exited with SIGSEGV. Isolated rerun and twenty stress repeats
  passed. Inspection found its single-thread assumption unenforced despite unsafe
  process-environment mutation. Every case now holds the same mutex through
  host teardown and environment restoration; this fixes that confirmed fixture
  race. The exact SIGSEGV cause was not independently established. The failed
  run and subsequent results remain separate receipts.

- Full workspace verification and the first MSRV/acceptance reruns encountered
  **ENOSPC** in generated compiler artifacts. Deleted only incremental-cache
  directories older than one day (3,043 directories), freeing about 51 GiB;
  kept recent caches, source, user state and built binaries. Initial failures
  remain in the archive; subsequent results are separate receipts.
- No release tag, production `main` push, Registry update, live provider request,
  credential change or local installation was performed for this audit.
- Historical private runs remain historical; the separately identified current
  run 37277822045 is settled and passed. Earlier Windows CRLF failure, stale
  production-library probes, an incorrect fixture tool name and a partial-probe
  query race are retained separately from successful receipts.
- PRD section 32 production workflow completion and actual tag/assets validation
  require the next authorized release. They cannot be certified by syntax,
  mocked publication or a private non-publishing fixture.
- Real-provider repair, interactive both-host observation and any unexecuted
  platform condition remain explicitly unverified; no numerical “100%” claim.

## DOX pass and readiness effect

Updated the nearest harness, crate dependency, app host, CI, xtask and documentation
contracts; indexed this report and superseded conflicting completion statements.
No domain boundary moved and no new child AGENTS was needed. Root AGENTS remained
unchanged by this audit: its existing RRC/exact-commit/completion contracts already
cover the repair. App parent, domain, fixture, registry, skills, installer and ADR
contracts remain unchanged because their durable responsibilities did not change.

The repaired controller passed the identified local and native fixture acceptance. It is
**not yet eligible for full PRD completion or production release** on local
checks alone. The candidate's required production exact-commit release gates
remain mandatory and unchanged.

## OpenAI sign-in follow-up

Alex additionally reported no browser opened and supplied a device-code screen
with no verification link. Production TUI `auth_settings::login` discarded the
URL whenever the callback carried a code, skipped browser launch, and left the
requesting status unchanged. The native OpenAI credential port already supplies
both the fixed verification URL and code; no adapter protocol change was needed.

The shared descriptor-driven TUI now validates and retains either challenge URL,
requests browser launch, displays the device verification URL/code, and keeps
Enter retry/C copy. Launch errors are explicit; successful process launch claims
only a launch request. Device fallback is limited to browser login and methods
that advertise it. Cancelling retains the previous credential. Regression restores
the old URL-discard behavior: **one test failed**; repaired behavior passes for
successful and refused launch hooks, complete URL copies and credential retention.

[Official OpenAI authentication documentation](https://learn.chatgpt.com/docs/auth)
requires opening the device verification link and entering its code. That source
is evidence of the user-facing flow, not third-party protocol stability or live
Vesper account entitlement. No screenshot code was used or recorded in receipts.

RRC repair factories use the active provider session; absent/expired OpenAI
credentials can prevent its model repair stage. GitHub gate/log collection uses
its separate GitHub adapter. The discarded sign-in link is a host UI bug; there
is no current failing RRC epoch receipt tying Alex's specific RRC failure to
credentials. ACP uses protocol-native challenge delivery rather than launching a
local GUI browser; no equivalent URL-discard branch was found in its path.
Live account completion and actual browser observation remain unexecuted.
