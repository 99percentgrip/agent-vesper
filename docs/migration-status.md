# Migration Status

Last updated: 2026-07-30

| Stage | Status | Evidence |
| --- | --- | --- |
| Reconnaissance and architecture | COMPLETE | `docs/recon/` |
| Stage 0 blocker resolution and fixture foundation | COMPLETE | `docs/foundation/`; 879 Python tests and 65 fixtures |
| Stage 1 Rust workspace foundation | COMPLETE | Six foundational crates, xtask, accepted ADRs, local verification |
| Stage 2 domain and oracle expansion | CI VALIDATION PENDING | Complete shared contracts; 76 scenarios; local verification |
| Stage 3 GLM provider adapter | CI VALIDATION PENDING | Production adapter; all 21 GLM fixtures pass locally |
| Stage 4 ACP adapter and minimal runtime | CI VALIDATION PENDING | Official SDK adapter, ephemeral actors, 12 real-process transcript tests, and all seven Stage 4.1 blockers closed locally |
| Stage 5 Part 1 session persistence setup | COMPLETE | Read-only ports, layouts, safe filenames, bounded non-recursive discovery |
| Stage 5 Part 2 legacy decoding and metadata | COMPLETE | Seven fixture outcomes, strict bounds, sidecar fallback, exact cwd filtering |
| Stage 5 Part 3 runtime conversion, identity, and replay | COMPLETE | Pure runtime-state seeds, deterministic IDs, visible-history filtering, writer-acknowledged ACP replay |
| Stage 5 Part 4 runtime/ACP read integration | COMPLETE | Actor-first composite reads, Agent Vesper format decoder, sanitized ACP lifecycle, bounded opt-in composition |
| Stage 5 Part 5 security and process invariants | COMPLETE | Adversarial bounds/redaction/concurrency plus 11 real-process disk-invariance vectors |
| Stage 5 Part 6 governance and coverage | CI VALIDATION PENDING | 76-scenario coverage, reusable testkit stores, writer/SQLite gates, 151 tests on Rust 1.95/1.88 |
| Core loop, tools, policy integration | NOT STARTED | Contracts only |
| Persistence writes, SQLite search, context, checkpoints, MCP, workers | NOT STARTED | — |
| Memory, skills, automation, plugins, observability | NOT STARTED | — |
| TUI/CLI and packaging | NOT STARTED | — |
| GLM parity gate | NOT STARTED | — |
| Multi-provider production expansion | BLOCKED | Blocked by GLM parity policy |

Stage 4 provides a deliberately minimal ACP executable for ephemeral no-tools
provider turns. It is not the complete harness: persistence, agent/tools, and
frontends remain unimplemented. Linux x86-64 is the only locally exercised host;
the five-target GitHub matrix remains unexecuted.

Stage 5 provides read-only discovery/decoding, runtime-state conversion, stable
compatibility identities, actor-first repository injection, ordered ACP replay,
adversarial security/concurrency tests, and real-process disk invariance. It
does not write persistent state, repair or migrate records, or search with
SQLite.
