# Disposable ACP Rust protocol-v1 spike

This package is migration evidence, not Agent Vesper implementation. It pins
official `agent-client-protocol` crate 2.0.0 and uses stable ACP wire protocol
v1. The source-required session-fork type currently requires the crate's
`unstable_session_fork` Cargo feature.

Run `cargo test --locked`.
