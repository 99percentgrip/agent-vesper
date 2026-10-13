# MessageJournal UX reconciliation

## Objective

Selective compatibility on canonical `MessageJournal` at `origin/main` `5a3ca226734a22a1ba8ac7a2a97100befdf63085`. The isolated worktree was `/home/Alex/Projects/agent-vesper-messagejournal-audit`, clean before edits. `FollowUpQueue` was not ported. Enter still queues. `/steer` is still explicit. `/queue resume` and interrupted-active recovery are unchanged. The dirty primary checkout was not modified. No commit, push, tag, or RRC run.

## Methods

- Read TUI `AGENTS.md` and `messages.rs`, `main.rs`, `ui.rs`, clipboard, and LM Studio session code on that SHA.
- `cargo test -p agent-vesper-tui --offline --lib messages::`
- `cargo test -p agent-vesper-tui --offline --bin agent-vesper-tui -- message_queue`
- `cargo test -p agent-vesper-tui --offline --lib show_more_expands`
- `cargo test -p agent-vesper-tui --offline --lib outstanding_http`
- `cargo test -p vesper-checkpoints --offline clipboard`
- `cargo clippy -p agent-vesper-tui -p vesper-checkpoints --offline --all-targets -- -D warnings`
- Local PTY driver `/tmp/journal-ux-demo/drive.py` against `target/debug/agent-vesper-tui` and a localhost LM Studio stub. No external account.

## Files

- `apps/agent-vesper-tui/src/messages.rs` — submitted-prompt collapse helpers and journal status lines.
- `apps/agent-vesper-tui/src/main.rs` — workflow refusal, immediate journal projection, promotion/settlement update, Show more keys.
- `apps/agent-vesper-tui/src/ui.rs` — render-time Show more / Show less and Copy full prompt. Stored lines are not rewritten.
- `apps/agent-vesper-tui/src/lmstudio_provider.rs` — cancellation during HTTP send and SSE read.
- `crates/vesper-checkpoints/src/clipboard.rs` — stdin closed before `wait`.
- `apps/agent-vesper-tui/AGENTS.md` — local contract for the projection.

## Queue lifecycle

```text
Enter or Tab with a non-empty composer while a turn runs
  -> MessageJournal admit
  -> one user: row with the exact body
  -> one journal-status row: Queued
  -> persist, or roll back and do not say QUEUED
Workflow pending_prompt while a turn runs
  -> not admitted; draft restored
/steer
  -> steering channel only
Turn success
  -> Active status becomes Settled on the same row
  -> oldest Pending promotes; its status becomes Processing; no second user row
Failure, cancel, disconnect, or reload of Active
  -> Interrupted; journal stays paused; /queue resume starts only pending ids
```

## Evidence

- Message journal library tests: 7 passed.
- Binary `message_queue` tests: 23 passed, including immediate body, one user row, Processing then Settled, and the next row still Queued.
- `show_more_expands_the_stored_prompt_without_rewriting_it`: passed. Collapsed view has Show more and Copy full prompt and hides the repeated detail. Expanded view has Show less, `fn kept`, and at least 10 `detail-line` copies. The stored line is unchanged.
- `outstanding_http_request_observes_cancellation`: passed in about 0.22s with `LM Studio request cancelled`.
- Clipboard tests: 3 passed, including a `read` that returns only after stdin EOF.
- Clippy `--all-targets -D warnings` for `agent-vesper-tui` and `vesper-checkpoints`: exit 0.
- PTY integration `terminal_message_queue_and_paste_lifecycle`: passed. The saved `display_transcript` does not contain `SPEC_START`. `/paste` still reveals that payload. Short queued text remains an exact `user:` row.

## AT matrix

| Id | Result | Note |
|---|---|---|
| AT-01 | PASS | PTY showed the exact queued body and Queued status while the prior turn was held. |
| AT-02 | PASS | FIFO tests still promote in order. Status becomes Settled only after success, and that sentence does not claim task success. |
| AT-03 | PARTIAL | Cancel still pauses and does not start pending work. LM Studio now observes cancellation. Wording remains Interrupted, not Delivery uncertain. No Ctrl+R retry. |
| AT-04 | DIFFERENT BY DESIGN | Reload still does not replay Active. Resume is `/queue resume`, not Empty Enter. |
| AT-05 | PASS | `/status` is not admitted. Covered by the existing command-plane tests. |
| AT-06 | PASS | Render test: Show more, code fence, and hidden repeated detail. Stored body unchanged. |
| AT-07 | PARTIAL | Copy full prompt is a visible control and Ctrl-Shift-C when a prompt is focused and nothing is selected. PTY did not execute the copy. Clipboard EOF is fixed and unit-tested. |
| AT-08 | PARTIAL | Empty-composer Tab focuses Show more (PTY). Enter toggles in code. PTY did not show Show less. Tab with text still queues. |
| AT-09 | PARTIAL | Reload keeps the journal body and can Show more. Full-body copy after reload was not executed in the PTY. |
| AT-10 | PASS | Order preserved. Settled is not a task-success claim. |
| AT-11 | DIFFERENT BY DESIGN | Failure still pauses. `/queue resume` starts future pending ids only. The interrupted active message is not retried. |
| AT-12 | PASS | `cargo xtask verify` exited 0 after the collapsed durable transcript fix. The log was truncated in the tool capture after the TUI PTY suite passed; the process exit was success and no cargo/xtask process remained. |

## Limitations

- No commit. The worktree is dirty relative to `5a3ca226`.
- Enter does not steer.
- `/paste` still exists for explicit inspection. It is not required to read a newly submitted body.
- Composer chips for unsent multiline or 1,000-scalar pastes are unchanged.
- Exactly-once provider side effects are not claimed.
