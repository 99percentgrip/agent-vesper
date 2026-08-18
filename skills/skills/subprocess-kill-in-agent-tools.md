---
name: subprocess-kill-in-agent-tools
description: Subprocess kill semantics in vesper-agent tool executors — avoid the orphaned-grandchild pipe-block under #![forbid(unsafe_code)].
version: 1.1.0
author: Agent Vesper library (migrated from legacy GLM-ACP)
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [subprocess, run-command, tools, timeout]
---

# Subprocess Kill In Agent Tools

When a tool executor spawns a shell via std::process::Command (e.g.
run_command runs `sh -c "<cmd>"` / `cmd /C <cmd>`), killing the child on
timeout/cancel with `child.kill()` kills ONLY the shell leader — not its
spawned grandchild (e.g. the `sleep` in `sh -c "sleep N"`).

The trap: the grandchild inherits the piped stdout/stderr, so a subsequent
`child.wait_with_output()` BLOCKS until that grandchild exits — turning a
1s timeout into a 30s hang.

Because the crate is `#![forbid(unsafe_code)]`, there is NO safe
killpg/process-group kill. The correct pattern
(crates/vesper-agent/src/tools.rs::run_bounded):

- Poll `child.try_wait()` in a loop with short sleeps (e.g. 25ms),
  checking deadline + cancellation.
- On the EXITED branch only: call `child.wait_with_output()` to read the
  now-closed pipes (safe — no grandchildren hold them).
- On the TIMEOUT/CANCELLED branch: `child.kill()` then `child.wait()` to
  reap the leader, then RETURN EARLY WITHOUT reading the pipes. Drop the
  child. Append a "[timed out and was killed]" string yourself. The
  orphaned grandchild lingers briefly and exits on its own — acceptable;
  do not block on it.
- Run the synchronous spawn/poll inside `tokio::task::spawn_blocking` so
  the async worker isn't blocked.

Tests: keep the test sleep short (e.g. `sleep 5` with a 1s timeout) so any
lingering grandchild exits fast and the suite stays quick.

## Provenance

Learned in the legacy GLM-ACP agent on 2026-08-01; migrated to the Agent
Vesper library.
