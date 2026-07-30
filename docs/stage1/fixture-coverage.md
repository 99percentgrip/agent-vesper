# Stage 1 Fixture Coverage

Status: COMPLETE

Rust loads and validates every authoritative language-neutral fixture without
rewriting the corpus. `cargo xtask fixtures verify-index` independently computes
SHA-256 for the exact canonical payload set.

| Category | Scenarios | Parsed | Schema validated | Foundational contract implemented | Runtime behavior deferred |
| --- | ---: | ---: | ---: | ---: | ---: |
| ACP | 12 | 12 | 12 | 0 | 12 |
| Provider/GLM | 21 | 21 | 21 | 0 | 21 |
| Sessions v1 | 7 | 7 | 7 | 3 | 4 |
| Tools | 10 | 10 | 10 | 0 | 10 |
| Policy | 6 | 6 | 6 | 6 | 0 |
| Security | 5 | 5 | 5 | 3 | 2 |
| Process | 4 | 4 | 4 | 0 | 4 |
| **Total** | **65** | **65** | **65** | **12** | **53** |

Implemented here means the applicable Stage 1 contract—not the full scenario
runtime—is represented and exercised. Examples include reasoning retention
boundaries, unknown extension preservation, the full policy precedence matrix,
canary-safe sinks, promptware delimiting, and secret redaction. ACP, GLM
transport, session persistence I/O, tools, plugins/checkpoints, and process
execution remain with their owning stages.

The machine-readable source of truth is
[`fixtures/coverage-stage1.json`](../../fixtures/coverage-stage1.json), generated
deterministically by `cargo xtask fixtures validate`.

## Non-normalizable invariants

Array/event order is preserved. Comparison helpers do not erase linkage,
terminal outcomes, hashes, redaction, or cancellation categories. Normalization
requires explicit collision-free literal rules. The Python descendant leak in
`tool.command-cancellation` remains recorded as a negative security fixture and
must not be reproduced.
