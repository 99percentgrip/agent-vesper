# Provider ports

## Purpose

Own provider-neutral request, capability, factory/session/catalog, stream,
continuation, fallback, and error contracts.

## Local Contracts

- This crate implements no concrete provider and depends only on `vesper-domain`.
- SDK, HTTP, process-runtime, authentication, and core-loop types are prohibited.
- Capability fallback is typed and observable.
- Streams must have ordered events, exactly one terminal state, explicit
  visible-output tracking, and no plain replay after visible output.
- Provider cancellation views are owned and remain usable for the lifetime of a
  returned stream.
- Explicit unsupported controls fail during request validation before dispatch.

## Verification

- Run `cargo test -p vesper-provider`.
- Run `cargo xtask architecture`.

## Child DOX Index

No children.
