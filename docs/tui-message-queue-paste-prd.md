# Reliable TUI message queue and collapsible pasted content

## Objective and boundary

Implement the 2026-10-12 user mission in terminal input, composer, command
classification, submission, normal/direct/ReAct/VRO settlement, transcript and
session recovery. Reuse the existing agent pathways and atomic session writer.
Source is canonical `1253ff8bfb89e23195557a6397783d36ed4209df`, in a fresh isolated
worktree. Initial implementation excluded publication. Alex subsequently
authorized release after the final audit, through native RRC and exact-SHA gates.
Installation, VRO-19, primary-checkout cleanup and automatic rebase remain excluded.

## Responsibility and data contracts

- Conversation: canonical original submitted bodies in the message journal;
  materialized provider history remains owned by the existing agent pathways.
- Command: registered typed command intents, approvals, cancellation, queue
  operations and explicit steering. Unsupported slash commands fail visibly.
- Queue: pending IDs, one active ID, FIFO and lifecycle authority in `messages.rs`.
- UI: draft anchors, cached chips, bounded previews, runtime indicators and the
  explicitly opened scrollable content projection. Rendering never dispatches.

One body plus ordered typed paste byte ranges represents text/paste segments
without duplicate segment payloads. IDs, metadata, lifecycle and queue order are
persisted. The queue stores IDs, not another body copy. Existing provider history
materialization is necessary for runtime/compaction; originals remain inspectable
when provider context changes. Metadata does not infer provider capabilities.

Terminal paste events retain exact delivered Unicode and line endings. Two or
more logical lines or at least 1,000 Unicode scalars produce a chip; shorter
single-line pastes remain editable. No extra delimiter or trimming enters ordinary
submitted message bodies. Backspace removes only the paste at its current boundary
or one typed scalar. The 1 MiB text and 16 MiB session limits fail visibly.

## Queue lifecycle and command surface

Enter during a turn acknowledges a durably stored future user turn, then clears
only the submitted draft. Tab retains the queue shortcut outside command menus.
Successful settlement commits history/final transcript before FIFO promotion.
Promotion persists the active ID and compact user turn before external work.
Only one normal agent turn runs; repeated redraws cannot dispatch twice.

Approvals keep pending turns waiting. Cancellation, terminal error, disconnected
worker, interrupted delegated execution, exit and crash preserve future entries
and pause automatic dispatch. `/queue resume` starts only pending future turns;
uncertain active work is archived as interrupted and is never blindly replayed.
Exactly-once local dispatch is required during normal operation. Exactly-once
remote effects across arbitrary crashes are not claimed.

| Command | Contract |
| --- | --- |
| `/queue` | State, IDs, bounded previews and paste metadata |
| `/queue page <n>` | Additional pages of pending previews |
| `/queue drop <id>` | Remove one pending entry; active work is not droppable |
| `/queue clear` | Remove all pending entries, preserving active work |
| `/queue resume` | Explicitly release a settled interruption/recovery pause |
| `/steer <guidance>` | Existing active-turn inbox; never implicit queueing |
| `/paste <message-id\|paste-id>` | Full inspection; arrows/pages scroll, Left/Right pan, Esc collapses |

Submitted pastes stay collapsed in normal conversation. Complete bodies remain
inspectable; inspection identity and scroll survive persistence. IDs and pending
order survive resume. Failed admission retains the draft without acknowledging;
failed promotion retains its pending ID and pauses. Failed queue mutations restore
the prior pending payloads. Legacy records load without fabricating original paste
boundaries: available large user bodies are labelled submitted content and remain
inspectable. Historical composition lost by older versions cannot be reconstructed.

These composer/inspection gestures are a terminal interaction contract. ACP
clients own their draft and protocol-request presentation/scheduling; no shared
slash catalog, provider adapter, RRC or core runtime behavior is changed.

## Required regression ledger

No row is complete without current scoped evidence in the
[execution report](foundation/2026-10-12-tui-message-queue-paste-execution.md).

| # | Required proof |
| --- | --- |
| 1 | Short single-line paste stays editable |
| 2 | Multiline paste collapses |
| 3 | Long single-line paste collapses |
| 4 | Unicode thresholds and logical-line rules are deterministic |
| 5 | Complete terminal-received content survives submission |
| 6 | Mixed typing/multiple paste order is exact |
| 7 | Submitted pastes remain compact in conversation |
| 8 | Complete content can be inspected |
| 9 | Collapsed/inspection state survives persistence/resume |
| 10 | Mid-turn ordinary Enter queues rather than steering |
| 11 | Two future messages execute FIFO |
| 12 | Slash commands bypass the conversation queue |
| 13 | Inspect/drop/clear operations preserve queue authority |
| 14 | Cancellation preserves and pauses future messages |
| 15 | Provider error preserves and pauses future messages |
| 16 | Explicit resume selects the correct pending ID |
| 17 | Pending approval prevents promotion |
| 18 | Normal/tool-heavy/ReAct/VRO successful settlement advances safely |
| 19 | No concurrent normal turns |
| 20 | No duplicate dispatch from redraws/completion notifications |
| 21 | Queue payloads, segments, order and IDs persist |
| 22 | Restart never blindly replays uncertain active work |
| 23 | Queued large pastes combine complete data and bounded UI |
| 24 | Unicode/newline fidelity |
| 25 | Ordinary rendering does not scan/render attachment payloads |
| 26 | Runtime and queue counts remain truthful and independent |

Focused unit/channel and real PTY/event-loop evidence precede sequential
`cargo xtask acceptance`, `cargo xtask verify`, `cargo xtask msrv`,
`cargo deny --all-features check`, `cargo audit`, format and whitespace gates,
with two Cargo jobs/two Rust test threads. Environmental blockers mean NOT READY.
Final audit must cover ownership, steering, queue loss, cancel/error loops,
serialization, uncertain replay, rendering costs and unrelated changes.
Commit only after all required gates pass. The later release authorization requires
native RRC admission and complete exact-SHA hosted gates before publication.

While a tool approval is waiting, **Ctrl+P Commands** opens queue management,
content inspection and informational commands without granting approval.
**Esc** returns to the approval dialog and restores the original draft.
Normal text in this command-only view is not submitted or queued.

Terminal text paste remains literal, including existing image-file paths.
Use the native clipboard image route or `/image` to attach an image explicitly.

## Current acceptance status

Initial implementation passed seven gates, 165 exact acceptance cases and 31
focused checks. The subsequent audit found native-command regressions; the corrected final source passed all seven mandatory gates and 35 scoped
regressions, including the strengthened real terminal interaction test.
READY FOR LIVE ACCEPTANCE: YES; publication remains pending native exact-SHA gates.
The execution report maps the 26 required behaviors to current automated evidence.
Native Windows/macOS and live-provider/delegated acceptance remain unexecuted;
local readiness does not claim those live proofs. Source, methods, failures and
verification receipts are retained in the linked execution report.

## Native command boundary

Ordinary conversational turns enter the queue. Plain native release imperatives
and slash commands retain their existing command-plane routes while a turn runs.
Enter invokes native admission; Tab refuses to queue commands and retains the
draft. A persisted command-shaped entry remains pending and paused with an
explicit drop/resubmit instruction; it never reaches the provider. Literal
attachment contents cannot introduce a command, including when later typed text
begins with a slash. Classification uses the typed prefix before the first paste.

The authorized release final audit found and reproduced this boundary regression
after initial local acceptance. Its corrective verification supersedes the prior
readiness claim. All required local gates passed on the corrected source;
publication remains pending native release and complete exact-SHA hosted gates.
