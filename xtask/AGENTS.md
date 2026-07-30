# Repository maintenance task

## Purpose

Own non-runtime commands for verification, fixtures, contract conformance,
architecture, MSRV, and source-oracle checks.

## Local Contracts

- `xtask` may depend on `vesper-testkit`; production crates may not depend on it.
- Commands must not call providers or mutate source/user state.
- Verification failures return nonzero and never fabricate success.
- Platform status distinguishes local execution from CI-pending evidence.

## Verification

- Run `cargo xtask architecture`.
- Run `cargo xtask fixtures validate`.
- Run `cargo xtask fixtures verify-index`.
- Run `cargo xtask fixtures coverage --stage 2`.
- Run `cargo xtask contracts verify`.
- Run `cargo xtask fixtures coverage --stage 3`.
- Run `cargo xtask provider glm verify`.
- Run `cargo xtask runtime verify`.
- Run `cargo xtask acp verify`.
- `acp verify` must include both the baseline transcript suite and Stage 4.1
  blocker process suite.
- Run `cargo xtask fixtures coverage --stage 4`.
- Run `cargo xtask fixtures coverage --stage 5`.
- Run `cargo xtask sessions verify`.
- Run `cargo xtask verify`.

## Child DOX Index

No children.
