# Disposable Rust SSE transport spike

This is not a GLM adapter. It validates current reqwest 0.13.4, a bounded
harness-owned SSE parser, explicit timeouts, cancellation, partial output, and
`Retry-After` behavior against local deterministic streams.

Run `cargo test --locked`.
