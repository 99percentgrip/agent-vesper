# Policy decision model

## Purpose

Own the pure policy/permission algebra and nested-workflow closure.

## Local Contracts

- Deny is absolute and Bypass never overrides it.
- Read Only never authorizes destructive or discovered MCP operations.
- Plan Mode preserves the source-compatible generic MCP allowance.
- Approval-channel failure denies.
- Smart review is advisory evidence and cannot independently expand authority.
- Providers and frontends never evaluate policy.

## Verification

- Run `cargo test -p vesper-policy`.
- Run `cargo xtask fixtures validate`.

## Child DOX Index

- `firewall/` — VRO-13 PR-1 hard-denial firewall: shell normalization, compiled rules, scan verdicts (pure, inert until PR-2 wiring).
