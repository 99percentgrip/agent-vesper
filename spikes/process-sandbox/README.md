# Process and sandbox conformance spike

Status: disposable. Promotion requires a later architecture review.

This crate exercises process-group ownership, cancellation, timeout, bounded pipe
draining, descendant cleanup, and Linux Bubblewrap namespace behavior. Platform
scripts provide real macOS and Windows CI entry points; their results are not
inferred from Linux.

Run locally:

```bash
cargo test --locked
```
