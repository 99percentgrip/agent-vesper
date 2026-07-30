# Migration Risk Register

Status: COMPLETE

## Scale

Likelihood and impact: Low, Medium, High, Critical. A migration gate is a condition that blocks advancing the affected subsystem.

| Risk | Likelihood | Impact | Evidence | Mitigation | Detection | Migration gate |
|---|---|---|---|---|---|---|
| Feature omission | High | High | ~28K production lines and 879 collected tests; many features coordinated inside `agent.py`/`tui.py` | module/command/tool/store matrices and traceable stage backlog | completeness audit; Python/Rust scenario coverage | no subsystem leaves parity backlog without explicit retire ADR |
| Hidden coupling | High | Critical | `agent.py:56-145` imports almost every service; loop performs policy, checkpoints, diagnostics, learning, hooks | ports/events/session actor; characterization tests before split | dependency checks, event differential tests | core loop scenario suite passes without concrete frontend/provider |
| Persistence incompatibility | High | Critical | session schema 1 and many project/private stores | dual readers, dry-run migrator, backups, unknown-field policy | golden legacy corpus and downgrade/rollback tests | every legacy store decodes before first Vesper write |
| Session corruption/lost updates | Medium | Critical | atomic files but no cross-process revision; current last-write-wins (`session_store.py:193-229`) | actor serialization + file locks/revisions; never overwrite corrupt originals | crash/fault/concurrent-writer tests | zero destructive recovery and transactional save tests |
| Event-order regression | High | High | explicit reasoning/content/tool/usage order (`glm_client.py:665-801`, `agent.py:6759-6950`) | monotonic event envelope and golden transcripts | differential sequence comparator | exact ACP/provider fixtures green |
| Cancellation regression | High | Critical | active HTTP task cancel and mixed tool/process paths | hierarchical tokens, drop/close streams, process supervisor | cancellation at every boundary; survivor scan | zero post-cancel deltas/processes |
| Process leakage | Medium | Critical | POSIX group/Windows Job behavior (`tools.py:1975-2082`) | platform supervisor, kill/reap/drain; kill-on-drop backup | grandchild/pipe tests on real CI | all five targets prove cleanup |
| Security-boundary regression | Medium | Critical | policy/containment/scrubbing/signatures/checkpoints across modules | dedicated security/policy crates; capability handles; adversarial corpus | security invariant suite and secret canaries | no weakening; platform sandbox truth verified |
| Path TOCTOU | High | Critical | resolve then open creates race (`tools.py:1181-1192`) | descriptor-relative/openat-style APIs where possible | symlink swap race tests | mutating filesystem tools pass race suite |
| Cross-platform differences | High | High | Bubblewrap, Seatbelt, Job Objects and installer/TUI differences | platform modules and real matrix; honest capability descriptors | Linux/macOS/Windows integration jobs | each supported target passes relevant conformance |
| Provider abstraction leakage | High | High | GLM fields embedded in session/config/UI (`agent.py:384-389`, `config.py:433-633`) | qualified IDs, capability descriptors, opaque extensions | forbid provider imports in core; second-adapter spike | OpenAI-compatible mock works without core conditionals |
| Lowest-common-denominator API | Medium | High | GLM reasoning/preservation/vision/cache/continuation are advanced | Native/Emulated/Unsupported/Unknown capability model | adapter conformance with required/prefer/fallback | GLM advanced fixtures remain expressible |
| Duplicate agent loops | Medium | Critical | temptation from CLI runtime integrations | process-backed providers emit same normalized stream | architecture dependency lint and shared scenario suite | all providers execute through one turn engine |
| TUI feature/accessibility loss | High | High | 5,016-line TUI and 3,017-line tests | reducer-first port, command/binding matrix, TestBackend snapshots, PTY/accessibility tests | source command audit and user acceptance | every command accessible; terminal restore proven |
| ACP SDK churn/ordering | High | High | official Rust crate 2.0.0 recently changed while wire remains v1; callbacks can block dispatch | wrap SDK, pin exact version, protocol fixture layer | compile/API canary, wire differential | SDK spike passes source transcripts before adapter build |
| MCP SDK immaturity/churn | Medium | High | official `rmcp` has recent 1.x migration | isolate behind port, pin, recovery fixtures | stdio/HTTP conformance and upgrade canary | parity recovery/cancel/name routing green |
| Dependency immaturity/churn | Medium | High | cron/TUI/keyring/platform crates vary | ADR per dependency, minimum features, lock/audit/licenses | cargo-deny/audit, MSRV/build matrix | no unreviewed security-sensitive dependency |
| SQLite/FTS packaging | Medium | High | FTS5 required; five targets | bundled/system spike and rebuildable index | runtime FTS probe/package tests | search works in each distributable |
| Cron semantic drift | High | High | interval/cron/ISO/timezone/DST and claim semantics | golden fake-clock cases before crate choice | differential schedule/claim model tests | all v1 schedules and races green |
| Plugin signature/hash drift | Medium | Critical | exact schema/hash/Ed25519 trust (`plugins.py`) | byte-level compatibility fixtures; crypto-reviewed crate | known vectors/tamper/key substitution | existing signed packages verify identically |
| Checkpoint incompatibility | Medium | Critical | schema 1/2, Git OIDs, zlib, conflict rollback | isolated crate/legacy reader; injected fault tests | cross-create/read/rollback/migrate/GC | no overwrite on conflict; object hashes exact |
| Test gaps / implementation assumptions | High | High | broad unit suite but no language-neutral fixtures/real all-platform sandbox fuzz | fixture-capture stage before production modules | coverage matrix and mutation/fuzz results | high-risk subsystem cannot start without characterization |
| Packaging/install differences | High | High | five archives, aliases, checksum/provenance/uninstall contracts | release spike, installer golden tests, preserve-state manifest | clean VM install/update/uninstall | artifact/alias/checksum/state preservation pass |
| Performance assumptions | High | Medium | no mission baseline; remote latency dominates | execute baseline plan before claims | statistical benchmark reports | no “Rust is faster” release claim without data |
| Excessive global/shared locking | Medium | High | Python session lock could become shared `Arc<Mutex>` | actor ownership, immutable snapshots, bounded channels | contention benchmarks/loom on primitives | session concurrency and TUI latency budgets |
| Backpressure/memory growth | Medium | High | many tiny SSE/TUI events and large contexts | bounded channels/coalescing with non-dropping visible events | slow-consumer/large-stream tests | peak memory/event loss targets |
| Reasoning/privacy regression | Medium | Critical | preserved reasoning default-on and provider-specific opaque data | explicit privacy classification and secret-safe storage | persistence/export/telemetry canaries | no reasoning in forbidden sinks; product decision on default |
| LLM-generated implementation error | High | High | migration breadth and subtle invariants | small reviewed stages, evidence links, property/differential tests, unsafe-code review | code review, mutation/fuzz, static checks | no generated subsystem accepted without targeted tests |
| Source reference drift | Low | High | frozen commit/dirty untracked user file | record commit/status and never pull/modify | closeout Git diff/status/hash | source state matches initial |
| Scope explosion before parity | High | High | multi-provider goal may encourage parallel features | GLM parity gate before provider expansion; adapters may be fixture-only earlier | roadmap gate audit | no production multi-provider feature displaces GLM parity |

## Highest-priority blockers before production implementation

1. Language-neutral ACP/provider/session/tool/security fixture schemas and capture runner do not yet exist.
2. Official ACP Rust 2.0/wire-v1 integration spike is not complete.
3. Cross-platform process-tree/sandbox and SQLite FTS packaging spikes are not complete.
4. Product decisions are required before intentionally changing legacy store location, reasoning persistence default, Bypass/Plan MCP behavior, or TUI commands.

These permit planning and disposable spikes but block feature implementation that would bake in unverified contracts.

## Review cadence

Review at every migration gate. A risk closes only with linked evidence; lowered likelihood without a test remains open. Any new provider adds adapter-specific auth, stream, reasoning, tool, caching, safety and rate-limit risks to this register.
