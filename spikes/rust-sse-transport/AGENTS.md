# Rust SSE Transport Spike

## Purpose

Validate bounded reqwest transport and harness-owned SSE parsing against the
Python GLM fixture semantics.

## Ownership

- The package owns only a local test server, disposable parser, and tests.

## Local Contracts

- Bind loopback only; no provider credentials or remote calls.
- Set explicit connect/request/read timeouts and disable client retries.
- Bound line/body memory and emit no event after cancellation.
- Never retry/replay after visible output.

## Work Guidance

- Test arbitrary byte segmentation and delayed cancellation points.

## Verification

- `cargo test --locked`

## Child DOX Index

No children.
