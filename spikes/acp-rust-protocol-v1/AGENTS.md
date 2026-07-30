# ACP Rust Protocol-v1 Spike

## Purpose

Validate official Rust ACP SDK 2.0.0 against frozen protocol-v1 fixture needs.

## Ownership

- The Cargo package contains only schema/dispatch/ordering/shutdown tests and a
  fixture consumer.

## Local Contracts

- Pin crate 2.0.0 separately from wire protocol v1.
- Enable unstable session-fork only to test the source-required fork surface.
- Do not implement Vesper runtime or business logic.

## Work Guidance

- Treat dispatch-loop callbacks as ordering barriers; tests must demonstrate
  the deadlock hazard and safe spawned/session callback pattern.

## Verification

- `cargo test --locked`

## Child DOX Index

No children.
