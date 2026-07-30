# ADR 0007: Rust Support and Release Targets

Status: RECOMMENDED

## Context

The source ships Linux x86-64/aarch64, macOS x86-64/aarch64, and Windows
x86-64. Published `agent-client-protocol` 2.0.0 declares Rust 1.88.0.

## Decision

Set provisional MSRV 1.88.0, test it plus current stable, and retain all five
release families. Pin dependency versions so an upstream “latest stable” MSRV
policy cannot silently raise Vesper's MSRV.

## Consequences

Local proof covers Linux x86-64 only. ARM64/macOS/Windows remain CI pending.
The long-term MSRV cadence and best-effort-target policy require approval.

