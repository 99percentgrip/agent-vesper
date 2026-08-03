# Vesper observability

## Purpose

Own the opt-in, secret-safe trajectory recorder and bounded reliability
aggregates used by composed Agent Vesper hosts.

## Ownership

- `src/lib.rs` owns JSONL event bounds, field filtering, aggregation, and
  percentile calculation.

## Local Contracts

- Recording is disabled unless the host explicitly enables it.
- Prompts, message bodies, tool arguments/results, reasoning, paths, commands,
  and credential-shaped fields never enter the event file.
- The crate has no provider, ACP, runtime, session, MCP, or TUI dependency.
- Malformed rows are skipped and counted; they never abort a status view.

## Verification

- `cargo test -p vesper-observability`
- `cargo clippy -p vesper-observability --all-targets --all-features -- -D warnings`
- `cargo xtask architecture`

## Child DOX Index

No children.
