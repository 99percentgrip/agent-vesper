# Grok subscription usage — execution report

## Objective

Replace xAI `query_usage`'s fixed unavailable notice with the first-party
Grok-session billing lookup, mapped into shared `ProviderUsage`, without
changing API-key billing, login, or ordinary turns.

## Methods

- Refreshed `xai-org/grok-build` `extensions/billing.rs` and
  `manager/enrichment.rs`. The files are identical at pinned
  `f0e3be1100ef5252488e3be8bb0e91cf68d8c305` and refreshed
  `482711333c7195dc16a272777f86086d615e2afb` (613 and 299 lines).
- Official FAQ `https://docs.x.ai/grok/faq` Usage & Limits: one weekly pool,
  percentage used, product breakdown, Settings reset time, and extra usage
  credits when present.
- Billing request shape from that source: bearer plus `X-XAI-Token-Auth:
  xai-grok-cli`, `x-grok-client-version`, `x-grok-client-mode`, and
  `x-userid` from `GET /v1/user`. Vesper also sends
  `x-grok-client-identifier: agent-vesper`. No Grok CLI or browser credential
  store is read.
- Live read-only probe through the selected Vesper Grok-session credential.
  The probe prints only the rendered card.

## Files

- `crates/vesper-provider-xai/src/usage.rs` — parser
- `crates/vesper-provider-xai/src/transport.rs` — bounded lookup
- `crates/vesper-provider-xai/src/factory.rs` — drop account model cache on
  logout and authentication changes
- `crates/vesper-provider/src/usage.rs` — optional window detail
- `crates/vesper-provider-glm/src/adapter.rs` — detail field
- `apps/agent-vesper-acp/tests/xai_native.rs`
- `apps/agent-vesper-tui/tests/xai_usage_pty.py`
- `apps/agent-vesper-tui/tests/xai_usage_pty.rs`
- `docs/xai-provider.md`, `docs/vro18-native-xai-provider-prd.md`,
  `docs/architecture/recon_xai_native_provider.md`

## Evidence

`cargo test -p vesper-provider-xai --all-features --lib` — 56 passed.

`cargo test -p vesper-provider --lib usage` — 4 passed.
`cargo test -p vesper-provider --test usage_render_snapshot` — 3 passed.

`cargo clippy -p vesper-provider-xai --all-features --all-targets -- -D warnings` — clean.

`cargo test -p agent-vesper-acp --features integration-test-harness --test xai_native usage_queries_grok_billing_without_inference -- --exact` — ok.

`cargo build -p agent-vesper-tui --features integration-test-harness --bin agent-vesper-tui` then
`cargo test -p agent-vesper-tui --features integration-test-harness --test xai_usage_pty -- --nocapture`:

```text
PASS: TUI /usage queried Grok billing without inference
test xai_usage_process_queries_billing_without_inference ... ok
```

Live probe (`XAI_BILLING_PROBE=1`, same lib test, 0.83s), card only:

```text
Account:                         Grok account / SuperGrok (account allowance)
Unified Weekly allowance limit:  [████░░░░░░░░░░] 31% left (resets 19:36 on 2 Oct 2026)
Extra usage credits:             $0.00
On-demand:                       $0.00 used of $0.00 cap
Grok Build limit:                [████░░░░░░░░░░] 32% left
Grok Voice limit:                [██████████████] 99% left
```

No user id, token, header, or raw payload was printed. API, Chat, and Imagine
rows were absent, so they were not shown as zero. Context remained the
estimated window, not the allowance.

## Deviations

- Returned zero cent objects are shown as `$0.00`. Grok Settings may omit an
  empty extra-credit balance; this card keeps returned zero distinct from a
  missing field.
- The reset is the server period end in the host's local timezone.
- This session did not open grok.com Settings. The card above is the
  comparison candidate for the same account and period.

## User acceptance

PASS. Alex confirmed the updated `/usage` screen works. That acceptance is
not a new live login and does not freeze the account percentages or reset
date as test constants.

## Unresolved at packaging

- Release workflow IDs are recorded in the v0.24.3 closeout after the gates.
- The installed app is not replaced by this packaging step.
- Paid API live acceptance was not run.
- Audit 2 and Audit 3 remain separate.
