# Reliable message queue and collapsible pasted content

## Objective and source

Implement the user-authorized TUI message lifecycle mission without push, release, installation, RRC-state mutation or VRO-19 work. Canonical source: `1253ff8bfb89e23195557a6397783d36ed4209df`. Isolated branch: `feat/tui-message-queue-paste-20261012`. Initial worktree was clean. The dirty primary checkout is excluded.

## Initial source-grounded audit (before implementation)

| Boundary | Existing functionality reused | Confirmed gap |
| --- | --- | --- |
| Terminal / composer | `main.rs::ingest_pasted_text`, bracketed paste and editable short text | Collapse threshold is 256 rather than 1,000 scalars; pasted segments have no position or identity. |
| Submission | `take_composer_text`, command classification and `dispatch` | Pasted payloads append after all typed text with added blank lines; prompt parser trims content. |
| Queue | `queued_prompts` and shared `spawn_submitted_prompt` | `VecDeque<String>` has no IDs, lifecycle or durable storage; active Enter sends implicit steering; Tab retains typed draft. |
| Agent settlement | `drain_agent_event`, direct/ReAct/VRO terminal events | Main loop dispatches whenever `agent_running` clears, including cancellation, provider failure and disconnected channel. History persistence precedes final transcript settlement. |
| Command | Existing pure command registry, independent `/usage`, cancellation and approvals | No inspect/drop/clear/resume queue surface; explicit steering needs a distinct discoverable route. |
| Conversation / UI | Compact paste transcript, shared physical-line renderer, live runtime indicator | Aggregate chips discard individual identities; no full-payload inspection; character counting scans pasted payloads each frame. |
| Persistence / resume | Atomic temporary-file write, file sync, bounded JSON, legacy loading | `persist_tui_conversation` saves only textual conversation/search projection; queue and paste associations are absent. `load_tui_session` replaces IDs and cannot recover uncertain active work. |

## Methods and inspected sources

Read root/apps/TUI DOX and documentation/foundation contracts. Traced `Event::Paste`, `pending_text_pastes`, `queued_prompts`, `agent_running`, `steering_tx`, `agent_rx`, cancellation, command dispatch, `spawn_submitted_prompt`, `drain_agent_event`, composer labels, transcript projection, `persist_tui_conversation` and `load_tui_session`. Existing tests prove simple deque order and steering inbox order, but do not prove the requested queue lifecycle.

## Status / readiness

Initial implementation was locally verified at `ca6eac79`. The subsequent release
final audit found and corrected command-boundary regressions. The final source
passed all seven mandatory gates and 35 scoped regressions.
**READY FOR LIVE ACCEPTANCE: YES.** Native publication remains pending.
All seven mandatory gates passed sequentially on the final frozen source, with
31 focused regressions passing. Native Windows/macOS and live-provider proofs
remain explicitly unexecuted; this is not universal platform certification.
No push, release or installation was performed.

## Implementation and ownership

- `messages.rs`: one canonical body per submitted turn, cached ordered paste byte
  ranges, stable message/paste IDs, images, FIFO IDs, lifecycle and validation.
  Text between ranges represents typed segments; no invented delimiters are added.
- `commands.rs`/`dispatch.rs`: typed queue, inspection and explicit steering
  intents on the command plane. Informational commands retain existing routes.
- `main.rs`: active history-load refusal, durable acknowledgment before clearing the draft, persisted active
  promotion before external work, direct/ReAct/VRO settlement, interruption pause,
  legacy loading, model-preflight deferral and content inspection composition.
- `ui.rs`: positional chips, bounded queue/transcript projections, independently
  truthful runtime/queue state and an explicitly opened scrollable detail overlay.

Existing terminal input, atomic session writer, runtime agent pathways, approvals,
steering inbox, shared provider catalogs, command registry and theme/navigation
primitives are reused. No provider adapter, ACP runtime, RRC or VRO-19 source is
changed. ACP clients own composer display and protocol request scheduling; this
terminal interaction contract adds no shared slash-command requirement.

## Queue and content behavior

Enter during an active turn stores a future user turn. `/steer <guidance>` alone
uses the active steering inbox. `/queue`, `drop <id>`, `clear`, `resume` and
`page <n>` show bounded state and operate on pending IDs. Active work cannot be
dropped. Successful final settlement commits history and final transcript before
FIFO promotion; failed or cancelled terminal events retain pending work paused.
ReAct/VRO statuses are preserved through their production terminal bridge; partial,
failed, cancelled, budget/rate exhaustion and inconclusive are never success.
Approval-pending and an existing active owner independently prevent promotion. Approval keyboard handling takes precedence until the explicit Ctrl+P command view is opened; the actual request remains pending during command navigation.

Queued bodies and metadata are stored before acknowledgment. Failed admission
retains the entire composer; failed promotion rolls back its user projection and
ID; failed drop/clear/resume restores previous pending payloads. Subsequent drafts,
pastes and images remain separate during execution. A model-capability deferral
keeps the original ID and one transcript projection, without authorizing execution
through a display marker. Normal operation has one active owner and no redraw-based
dispatch. Interrupted active messages recover as `Interrupted`, not pending;
recovered pending messages require explicit `/queue resume`. Historical context is
preserved; an interrupted turn is not dispatched again. Exactly-once remote tool
side effects across arbitrary crashes are not claimed.

Paste collapse uses two logical lines or 1,000 Unicode scalars. CR, LF and CRLF
are preserved in the canonical body; CRLF counts once for metadata. Short
single-line text remains editable. Cached labels and anchored gaps preserve event
order and individual IDs. Backspace removes the attachment at the current boundary
or one adjacent typed scalar. Text over 1 MiB and session JSON over 16 MiB fail
visibly. These are terminal-delivered text guarantees, not clipboard-byte claims.

Normal transcript rows retain compact chips and `/paste <id>` inspection hints.
Inspection builds a complete scroll cache once when explicitly opened, with
vertical/page navigation and horizontal pan for wide glyphs; Esc collapses. The
inspection identity and scroll position persist. Ordinary frames see only bounded
cached projections, never attachment bodies. Legacy sessions cannot reconstruct
historically lost composition boundaries; available large stored user bodies are
labelled `Submitted Content`, remain inspectable, and retain historical activity.

## Red-before-green and interim findings

The original production `take_composer_text` function was extracted verbatim from
source `1253ff8bfb89e23195557a6397783d36ed4209df` into a focused Rust baseline.
Expected `AB\nCD\r\n`, actual `AC\n\nB\n\n\nD\r\n`; one test failed (exit 101).
Receipt: `/tmp/vesper-queue-red.log`. This is a source-extracted focused baseline,
not a claim that an unrelated old suite was run.

Initial new real-PTY verification failed because active queue-operation status was
hidden by the renderer; the production visibility defect was repaired. Initial
library runs exposed the now-expanded command count and a legacy-content label
assertion. Their contracts were updated while retaining their substantive checks;
new explicit command-name/identity assertions were added. A recovery assertion
incorrectly treated preserved historical context as replay; it now verifies the
newest user body and exact POST count. A one-shot HTTP 401 fixture exercised the
adapter's existing authentication retry instead of a terminal failure; HTTP 400
now exercises the intended queue pause without changing adapter behavior.
The first all-feature acceptance attempt found a missing completion-success
field in the Swarm parent's host constructor; that constructor was corrected and
all fifteen focused queue lifecycle tests passed with all features before the
mandatory ladder was restarted. The successful acceptance run passed all 165 cases. Verify then found a collapsible-if Clippy style error in inspection restoration; the syntax-only correction passed all sixteen focused lifecycle tests before Verify restarted. The next Clippy pass required moving the message module tests after production helpers; the seven message-model tests passed after that syntax-only rearrangement. The final delegated-cancellation bridge now retains partial output and requires the host token before presenting Cancelled; all seventeen lifecycle tests passed with all features before restarting the complete ladder on the final production source. The failed Verify log is retained at `/tmp/vesper-queue-gate-verify-attempt1.log`. Its failed receipt remains
`/tmp/vesper-queue-gate-acceptance-attempt1.log`.

## Regression evidence map

| Mission cases | Production-scoped evidence |
| --- | --- |
| 1–4 | `messages::tests::collapse_thresholds_unicode_and_newlines`; binary `production_composer_exact_event_order_and_boundaries`; existing paste regressions |
| 5–6, 24 | `mixed_events_preserve_exact_order`, production composer test and real PTY exact mixed body on the loopback provider wire |
| 7–8 | Compact production transcript and `/paste` overlay in real PTY; `full_inspection_collapsed_transcript_and_expanded_resume_are_preserved` |
| 9 | Inspection identity/scroll persistence test and real session restart/full-content inspection |
| 10–12 | Real Enter while held turn, second future turn, FIFO POST bodies and informational/queue commands without extra POSTs; explicit steering channel tests |
| 13 | Queue inspect/drop/clear tests, failed mutation rollback and active-drop refusal; real PTY pending drop |
| 14–16 | Production channel cancellation/error/disconnect tests and real Ctrl+C/HTTP400/pending resume PTY |
| 17 | `queue_approval_test::pending_real_approval_prevents_queue_promotion` with the actual approval broker |
| 18 | Successful normal/tool-heavy settlement test plus `production_react_and_vro_bridge_preserves_all_terminal_outcomes` using the same bridge as both production sources |
| 19–20 | Active-owner/100 repeated promotion checks, one transcript row, preflight deferral identity test and real FIFO POST count |
| 21–23 | Serialized payload/IDs/ranges, failed save rollback, actual process termination/restart, paused recovery and exact future-only dispatch PTY |
| 25 | 600 KB collapsed-payload preview test, structured chip no-refold test, cached frame projection/inspection render tests and real multiline paste interaction |
| 26 | `runtime_and_queue_projection_remain_independent`, real RUNNING/READY and queue-count terminal observations |

Current focused receipt `/tmp/vesper-queue-focused-final4.log`: 289 library tests
and 189 binary tests passed before the final preflight-display regression addition.
Current expanded PTY receipt `/tmp/vesper-queue-pty5.log`: PASS for mixed paste,
Enter queue, FIFO, commands, drop, inspection, compact transcript, IDs,
cancellation/error pauses, crash recovery and explicit safe resume. Final source
verification and updated totals follow below; interim receipts do not certify
later edits.

## Remaining live acceptance

No public provider call, real credentials, invoking user-state write, installation
or GitHub publication was performed. Local terminal proof is Linux POSIX PTY with
a synthetic loopback provider. Native Windows ConPTY/macOS interaction and live
provider/device timing remain unexecuted. Delegated completion evidence tests the
production host bridge, not a live multi-model VRO/ReAct run. These boundaries are
retained explicitly; local readiness is not universal platform/live certification.

## Final source audit and safety receipts

Source audit covers composer admission, command-only dispatch, one active owner,
normal/tool-heavy settlement, delegated terminal status, cancellation/error pause,
queue mutation rollback, legacy and malformed journal loading, uncertain recovery,
inspection cache, bounded projections, subsequent image drafts and preflight
identity. `/history <id>` refuses an active-session replacement, so it cannot
replace acknowledged pending turns. No normal redraw dispatches a message.
Only the explicit steering command sends to `steering_tx`.

All changed production paths are inside the TUI composition boundary. The Swarm
host change supplies the same completion-success receipt required by the updated
host event; it does not change the Swarm service or its execution policy.
Synchronous work on these paths is bounded paste ingestion, metadata creation,
explicit one-time inspection and the existing atomic JSON serialization/fsync.
Collapsed frame work skips body ranges and uses cached metadata. Durable admission
waits for the bounded session write before acknowledgment; slow/full storage fails
truthfully and is not represented as guaranteed disk latency. The real terminal
fixture proves input and queue operations against held provider work on this host.

Canonical `origin/main` was fetched again and remains
`1253ff8bfb89e23195557a6397783d36ed4209df`; no rebase was performed. Primary tracked
content/status digests remain the initial values:
`b197edc1eca3af982528bfda5dd5d02876f0e3ae39e5cd8d780779cd9261adb7` and
`3ded70c52125bf9f4b4ddfc9e57032ea7a660b25e7684329ea44da382afbecec`.

Nearest owning TUI/tests and documentation/foundation DOX contracts were updated.
Root/apps parent contracts remain unchanged because no provider/runtime boundary,
subtree or top-level child index changed. User guidance, this PRD and evidence
index are in the same candidate. No separate documentation push is authorized.

## Production and supporting files

Production: `apps/agent-vesper-tui/src/messages.rs`, `lib.rs`, `commands.rs`,
`dispatch.rs`, `main.rs`, `swarm_host.rs`, and `ui.rs`.
Regression integration: `apps/agent-vesper-tui/tests/message_queue_pty.py` and
`message_queue_pty.rs`; unit/channel/render tests remain beside production owners.
Documentation: owning TUI/tests DOX, docs/foundation DOX and docs ownership,
`docs/tui-message-queue-paste-prd.md`, `docs/using-vesper.md`, evidence index,
this execution report and its final verification receipts.

## Verification methods

Baseline: source-extracted original composer function compiled with
`rustc --test`; expected failing regression retained. Focused production proof:
`CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 cargo test -p agent-vesper-tui --lib --bin agent-vesper-tui`
and all-feature `message_queue_tests`; terminal proof:
`python3 apps/agent-vesper-tui/tests/message_queue_pty.py target/debug/agent-vesper-tui`
with an `integration-test-harness` binary. The all-feature suite enrolls the
same fixture through `message_queue_pty.rs`.

Mandatory commands run sequentially with `CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2`:
`cargo xtask acceptance`, `cargo xtask verify`, `cargo xtask msrv`,
`cargo deny --all-features check`, `cargo audit`,
`cargo fmt --all -- --check`, and `git diff --check`.
No expensive Cargo gates run concurrently. Gate results must be recorded below;
commands listed here alone are not proof of execution or success.

## Approval command-plane audit

The final interaction audit found that the existing permission modal intercepted
queue commands. Ctrl+P now opens a command-only view, preserving the original
text/paste/image draft by moving ownership into a transient backup. Queue
management and inspection remain available while the real broker request stays
pending. Esc restores the draft; Enter/Tab admission is rejected in this view.
The real-broker regression proves queue clearing does not grant permission.
The PTY fixture also exercises command access during a real hosted-tool approval
and subsequent tool-heavy completion. Final receipts distinguish executed proof
from unfinished checks. Active-turn future-message admission no longer waits on
background OpenAI discovery; normal execution still validates its adapter catalog.

The final cancellation audit also moved forced-interruption persistence after
partial assistant/trajectory projection. The existing cancellation regression now
checks the saved partial response and tool activity, alongside visible retention.

## Pre-correction focused regression receipts

The earlier implementation passed 17 lifecycle regressions, one actual
approval-broker regression, one persisted partial-cancellation regression, seven
message-model regressions, three rendering regressions and one real terminal
integration regression: **30 passed, zero failed**. The PTY check took 38.97 seconds
and includes approved private tool execution, command access during approval,
original draft restoration, FIFO promotion, cancellation, terminal error and
crash/restart recovery. Commands/results are captured in the final receipts JSON.
These are interim receipts; the final literal-path source and results follow below.

## Literal terminal image-path correction

The final fidelity audit found that existing image-path recognition could consume
terminal text instead of inserting it. `Event::Paste` now uses literal terminal
text ingestion. The native clipboard image/path route remains available and its
existing image-path regression is retained. A new production regression checks
an existing `.png` path as editable text and preserves a trailing CRLF; the real
PTY mixed-body assertion includes an existing image-file path. Prior frozen
receipts above predate this correction and do not certify the final source.

The scheduler for the previous verification attempt was stopped at its owned
Verify boundary after the literal-path correction. Its already-running Verify
child was allowed to finish and clean up; Cargo gates do not overlap. Those
pre-correction receipts are provisional. Focused regressions and the complete
mandatory ladder are rerun on the final frozen source before a commit.

The existing image-path regression keeps all its assertions and is renamed
`native_clipboard_text_queues_existing_image_path_without_touching_composer` to
identify the preserved route accurately. No regression is deleted or weakened.

## Initial implementation verification and readiness

[Machine-readable receipts](2026-10-12-tui-message-queue-paste-receipts.json)
record commands, results, log digests/tails, frozen source hashes, focused test
results, red-before-green evidence and existing ignored cases. All commands used
`CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2` and ran sequentially.

| Mandatory gate | Result | Seconds |
| --- | --- | --- |
| `cargo xtask acceptance` | PASS — 165 exact cases | 207.50 |
| `cargo xtask verify` | PASS — format, Clippy, all-feature workspace, contracts and acceptance | 949.61 |
| `cargo xtask msrv` | PASS — Rust 1.88 all-feature workspace | 772.10 |
| `cargo deny --all-features check` | PASS | 1.29 |
| `cargo audit` | PASS | 2.62 |
| `cargo fmt --all -- --check` | PASS | 1.70 |
| `git diff --check` | PASS | 0.04 |

Final focused checks: **18 lifecycle + 1 approval broker + 1 persisted cancellation
+ 7 message model + 3 rendering + 1 real PTY = 31 passed, zero failed**.
The final PTY scenario took 39.44 seconds and includes literal existing image-file
paths on the exact provider wire. Full Verify also passed 323 TUI library tests
and 196 TUI binary tests, with one existing explicit-container binary test ignored.
Other pre-existing ignored prerequisite/live/container cases are listed verbatim
in the receipts and are not counted as completion evidence.

Final source hashes match the freeze through gate completion. The owning DOX,
user guide, PRD, evidence index, report and JSON receipts are part of the same local
implementation commit. Post-gate prose updates receive content/link/JSON/whitespace
checks; no source changed and no additional program suite is required for prose.
The conversation delivery records the final commit SHA without a self-referential
report hash. No source drift was observed at final fetch. Primary tracked-content
and status digests remain unchanged.

**Initial READY_FOR_LIVE_ACCEPTANCE: YES**, superseded by the correction below.
Outstanding native Windows/macOS interaction,
real provider timing and live delegated VRO/ReAct proof remain listed above.
At initial implementation delivery, no user installation, publication, RRC
release or VRO-19 execution was performed. Subsequent release authorization and
its interrupted audit attempt are recorded below; installation and VRO-19 remain
out of scope.

## Authorized release final audit

Alex subsequently authorized one release after the mandatory final audit. The
implementation commit is `ca6eac7977262f066a8847cd98eb95ffdb9bc665`; canonical
`origin/main` remains `1253ff8bfb89e23195557a6397783d36ed4209df`. The candidate
was clean before recording this authorization; no automatic rebase occurred.
All nine source/test digests and seven executed gate-log digests match the
committed receipts. Source review rechecked journal admission/promotion, runtime
settlement, interruption pause, persistence rollback/recovery, bounded rendering
and explicit steering. That pass missed the native-command defects identified
in the continued audit below; its readiness assessment is superseded.

The 26-behavior evidence map remains scoped to its actual proof. Queue terminal
acceptance runs on Unix with an integration-only loopback adapter; the existing
five-target first-launch/authentication/RRC tests do not certify Windows queue
interaction. Windows/macOS queue interaction and live delegated timing remain
outstanding user acceptance, not inferred from unrelated platform gates.

Latest publication is v0.24.12; v0.24.13 has no remote tag or release at admission.
Release preparation, conservative sequential verification, exact-SHA CI, immutable
tag/assets and existing registry PR #539 closeout belong to the native RRC.
The invoking native host must remain attached through completion; no disposable
ACP driver with independent permissions substitutes for that route. Release
publication is pending, and no installed application replacement is authorized.
Owning DOX contracts were checked: this record and provenance update change no
subtree, message ownership or workflow contract, so those DOX files remain unchanged.

## Continued final audit: native command correction

The initial final-audit pass missed an unintended RRC routing change. During an
active turn, the new ordinary-message predicate admitted plain release imperatives
as queued provider work. Queue promotion called the normal coding pathway without
native release classification. Separately, Tab could acknowledge an unknown slash
command when its completion menu was empty. Both violate the command boundary.

A focused test extracted the exact production predicate into its live helper and
failed on `release patch` during an active turn (one test, exit 101). The correction
restores native natural-language admission during active turns, rejects commands
at both queue admission paths, and fences command-shaped persisted entries at
promotion with retained payload/ID and a pause. It also classifies slash authority
from the typed prefix before the first attachment so slash-containing literal
pastes and later typed paths cannot accidentally become commands. The promotion
fence runs only after the existing runtime/approval/pause guards, preventing
repeated persistence or payload classification on ordinary frames.

The original native candidate was cancelled with `/release cancel`, then `/quit`,
while local verification was incomplete. Candidate push and tag flags are false;
no publication occurred. The prepared 0.24.13 version remains only in that isolated
controller workspace. No journal clearing, retry-budget reset, manual release
mutation, installation or primary-checkout cleanup occurred. This operator audit
correction does not claim autonomous RRC source repair.

Red receipt: `/tmp/vesper-queue-native-route-red.log`. Current focused correction:
22 lifecycle tests pass; a real PTY proves command draft retention, native
non-Git admission instead of a provider POST, original FIFO/paste/approval and
crash/cancel/error behavior. The strengthened unknown-command Enter assertion passed, and the complete
sequential ladder passed on the final frozen source before commit and renewed
release admission. Earlier receipts remain historical and are not
used to certify these later changes.

DOX: nearest TUI/tests contracts, user guide and owning PRD record this command
classification boundary. Parent ownership and child indexes are unchanged.

## Corrective final verification and release readiness

[Corrective machine-readable receipts](2026-10-12-tui-message-queue-paste-native-command-receipts.json)
retain the red reproduction, exact final-source digests, 35 green scoped
regressions, strengthened real Linux PTY proof and all seven gate logs/digests.
Original implementation receipts remain historical. No test was weakened.

| Final-source gate | Exit | Seconds |
| --- | --- | --- |
| `cargo xtask acceptance` | 0 | 331.82 |
| `cargo xtask verify` | 0 | 1276.25 |
| `cargo xtask msrv` | 0 | 1061.16 |
| `cargo deny --all-features check` | 0 | 1.4 |
| `cargo audit` | 0 | 2.4 |
| `cargo fmt --all -- --check` | 0 | 1.75 |
| `git diff --check` | 0 | 0.02 |

The ladder ran sequentially with `CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2`
and the existing RRC-owned target cache. Acceptance executed 165 cases;
workspace verification and Rust 1.88 MSRV both passed the real terminal test.
All nine source/test hashes still match the frozen candidate. Existing ignored
tests are retained in the receipt and are not completion evidence.

Final source review rechecked canonical payload ownership, ordered paste ranges,
FIFO and one-active dispatch, command/steering separation, all terminal outcomes,
approval guards, admission/promotion rollback, persisted IDs/recovery, compact
rendering, and absence of unrelated provider/RRC source changes. No further
confirmed defect remains in this mission audit. This is scoped automated proof,
not a claim of universal absence of bugs or completed native Windows/macOS/live
provider acceptance. Those proofs remain explicitly outstanding.

The invoking native RRC will prepare the same unused v0.24.13 after this correction
is committed. Its prior attempt was unpublished and cancelled during audit.
Postpublication receipts remain local; no separate documentation push or user
installation is authorized. Parent DOX ownership/indexes remain unchanged;
nearest TUI/tests contracts, guide, PRD and evidence index are updated together.

**READY_FOR_LIVE_ACCEPTANCE: YES.** Publication still requires complete native
local gates, four exact-SHA hosted workflows, tag/assets and registry closeout.

Release capacity preparation removed 3,352 obsolete generated ELF executables
from the stopped RRC-owned `debug/deps` cache (timestamps before 2026-10-12),
raising available space from 108.83 to 150.70 GiB. Source, libraries, current
binaries, preserved host archives, journals, evidence and the dirty primary
checkout were retained. The detailed cleanup receipt is hashed in the corrective
receipt; resource thresholds and retry budgets were unchanged. Earlier retained
host archives and obsolete incremental-snapshot cleanup remain separate capacity
actions, not source repairs or verification proof.
