# MessageJournal final UX acceptance

## Objective

Verify the canonical MessageJournal reconciliation in the isolated worktree before any integration commit. No release, push, tag, or RRC.

## Base

Detached HEAD at `5a3ca226734a22a1ba8ac7a2a97100befdf63085`. The worktree is not a named branch. No commit was created. A commit was not separately authorized.

## Method

`apps/agent-vesper-tui/tests/message_journal_ux_pty.py` drives `agent-vesper-tui` built with `integration-test-harness` against a localhost xAI stub. The prompt is nine lines: a heading, a rust fence, four-space indentation, `💡`, and `MARKER-TAIL-UNIQUE`.

## Matrix

| Check | Result | Evidence |
|---|---|---|
| Long prompt starts collapsed | PASS | Screen shows `MARKER-TITLE` and Show more. `MARKER-TAIL-UNIQUE` is absent. `/paste` is not required. |
| Show more in the conversation | PASS | Empty-composer Tab focuses the control. Enter reveals the tail, `fn kept`, `    indented-keep 💡`, and Show less. |
| Show less | PASS | Enter again removes the tail from the current screen and returns Show more. |
| Restart and expand | PASS | `/quit`, new process, `/history <id>`, Tab, Enter shows the tail again. |
| Copy equals the stored body | PASS | Mouse click on Copy full prompt. `checkpoints/clipboard.log` equals the journal text plus a newline, before and after restart. |
| Two queued rows | PASS | While a turn is held, Enter and Tab each add one `user:` row immediately, both `Pending` and labeled Queued. |
| FIFO and identity | PASS | After release the journal order is the long prompt, the held turn, `QUEUE-ONE-EXACT`, `QUEUE-TWO-EXACT`, all Settled. The wire user texts match that order. Four user rows, no duplicates. |
| Enter/Tab do not toggle | PASS | Typing while Show more is focused does not expand. Queue Enter/Tab leave Show more on screen and do not report Prompt expanded. |
| Workspace verify | PASS | `cargo xtask verify` returned success. The new PTY test and `terminal_message_queue_and_paste_lifecycle` both passed inside it. |

## Code fence note

The fence is rendered as a rust code block (`fn kept() {}`), not as literal backtick characters. The stored journal body still contains the fence.

## Changed files

- `apps/agent-vesper-tui/src/main.rs`
- `apps/agent-vesper-tui/src/messages.rs`
- `apps/agent-vesper-tui/src/ui.rs`
- `apps/agent-vesper-tui/src/lmstudio_provider.rs`
- `apps/agent-vesper-tui/src/markdown.rs`
- `apps/agent-vesper-tui/AGENTS.md`
- `apps/agent-vesper-tui/tests/message_queue_pty.rs`
- `apps/agent-vesper-tui/tests/message_journal_ux_pty.py`
- `crates/vesper-checkpoints/src/clipboard.rs`
- `docs/foundation/evidence-index.md`
- `docs/foundation/2026-10-13-messagejournal-ux-reconciliation.md`
- `docs/foundation/2026-10-13-messagejournal-final-ux-acceptance.md`

## Not done

No integration commit. The worktree remains detached. Release remains unauthorized.
