# 2ai — `/usage` regression: root cause, fix, and evidence

**Date:** 2026-09-17 · **Increment:** 2ai · **Status: FIXED (client honesty) + EXTERNAL (account)**

## Objective

Alex reported `/usage` is "broken and no longer as before" — the panel used
to show live Z.ai Coding Plan windows (recorded in the Sep-10 session
`0c65f2bd`: 5h/weekly/monthly meters with reset times) but now fails.

## Investigation (systematic-debugging, all phases)

### Phase 1 — evidence

1. **Repository state:** 38 files uncommitted, but *none* touch the usage
   path (`vesper-provider/src/usage.rs`, GLM quota.rs, TUI usage plumbing
   all unchanged since 08-29 / v0.22.6). The regression is **not local code**.
2. **Unit level:** `vesper-provider` usage tests pass (3/3), GLM crate tests
   pass (59/59). Parser + renderer are healthy on valid envelopes.
3. **Live level (tight loop, `usage_pty.py`):** drove the REAL release TUI
   over a PTY, typed `/usage`, captured the actual screen:

   ```
   Z.ai · Usage
   Model:           glm-5.3
   Reasoning:       enabled
   Permissions:     Ask
   Context window:  100% left (1.1K used / 1M, estimated)
   Warning: Usage refresh failed: GLM returned malformed protocol data
   ```

   The panel machinery works end-to-end; the **quota query** fails.

### Phase 1 — the wire (decisive evidence)

Queried the real monitor endpoint from this machine:

| Request | Response |
|---|---|
| No auth | HTTP 200 `{"code":1001,"msg":"Authentication parameter not received…","success":false}` |
| Alex's key (from keyring) | HTTP 200 `{"code":500,"msg":"Internal service error","success":false}` ×3 |
| Same key on chat | **HTTP 429 `{"code":"1113","message":"Insufficient balance or no resource package. Please recharge."}`** |

### Root cause — two stacked facts

1. **EXTERNAL (the "as before" part):** the coding-plan balance behind the
   key is exhausted/expired. Chat itself is 429-blocked with code 1113. The
   Sep-10 panel worked because the plan was active then. Nothing in
   Vesper caused this and no client change can restore those numbers.
2. **CLIENT BUG (mine, fixed):** the monitor answers account failures with
   **HTTP 200 + explicit error envelope** (`success:false`, `msg`, `code`)
   and no `data.limits`. The old parser classified any envelope without
   `data.limits` as `MalformedProtocol`, so `/usage` blamed "malformed
   protocol data" instead of the provider's own failure reason. That is a
   truthful-reporting violation: the provider *told* us why; we hid it.

## Fix (red-first)

**Test first:** `crates/vesper-provider-glm/tests/quota_error_envelope.rs`
— 3 red (error envelopes must surface provider msg / auth text / honest
attribution) + 1 guard (valid envelopes still parse).

**Implementation:**
- `quota.rs`: `success:false` envelopes now produce
  `GlmAdapterError::MonitorError(bounded msg)` (24-word cap) instead of
  `MalformedProtocol`.
- `error.rs`: new `MonitorError(String)` variant; classified
  `quota-monitor`; provider message stays static-safe while the dynamic
  reason rides in the redacted-diagnostics field `zai:quota-monitor-msg`.
- `lib.rs`: `#[doc(hidden)] __quota_parse_for_tests` seam (no public API
  change).

## Verification receipts

```
quota_error_envelope (vesper-provider-glm)   4/4   (3 were red pre-fix)
vesper-provider-glm full                     59+4 passed / 0 failed
vesper-provider full (incl. usage render)    18+3 passed / 0 failed
usage_render_snapshot (new, panel contract)   3/3
agent-vesper-tui bin drain_usage tests        2/2
openai_native ACP integration (usage lane)    5/5
clippy -p vesper-provider-glm -D warnings     clean
cargo fmt --all --check                       clean
LIVE usage_pty.py on rebuilt release TUI      PASS
```

## Live screen after the fix (real binary, real endpoint)

```
Warning: Usage refresh failed: Z.ai quota monitor reported an account error
```

The user now sees an account problem named as an account problem.

## Remaining / manual action

- **Alex's Z.ai coding plan is exhausted** (`1113` on chat, monitor error
  envelope). `/usage` numbers return when the plan is recharged/active.
  That is a provider-account action, not a code change.
- The exact recharge path (Z.ai console) is outside this workspace's scope.

## Files changed

- `crates/vesper-provider-glm/src/quota.rs` — error-envelope classification
- `crates/vesper-provider-glm/src/error.rs` — `MonitorError` variant + mapping
- `crates/vesper-provider-glm/src/lib.rs` — test seam
- `crates/vesper-provider-glm/tests/quota_error_envelope.rs` — regression tests
- `crates/vesper-provider/tests/usage_render_snapshot.rs` — panel contract loop
- `apps/agent-vesper-tui/tests/usage_pty.py` — real-binary PTY loop
