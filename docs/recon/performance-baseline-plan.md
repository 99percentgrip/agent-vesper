# Performance Baseline Plan

Status: COMPLETE

## Principle

Rust performance claims require before/after measurements with identical fixtures. Separate remote model/network time from local harness, tool, persistence, and UI time. Report median, P95, P99, standard deviation, peak RSS, allocations where available, bytes/processes/files, and artifact size.

## Environment control

- Pin source commit, Python/uv lock, Rust lock/toolchain, OS image, CPU governor and terminal size.
- Isolate HOME/config/session directories; warm/cold page cache are separate runs.
- Disable live cron/MCP/telemetry unless the benchmark targets them.
- Use a local deterministic HTTP/SSE server with configurable chunk schedule; live provider runs are labeled separately.
- Use fixture repositories: tiny (100 files), medium (10K), large (100K with ignored trees/binaries/symlinks), and Git history/worktree/checkpoint variants.
- At least 30 local repetitions after five warmups; bootstrap confidence intervals.

## Measurements and repeatable commands

Commands are proposals for the implementation phase; they are not executed in reconnaissance.

| Metric | Python baseline method | Rust comparison / isolation |
|---|---|---|
| Cold CLI startup | `/usr/bin/time -f '%e %M' env -i PATH=... HOME=$TMP glm-acp --version` after clean process; do not claim disk-cache cold unless controlled | same binary/args; loader and artifact separated |
| Warm startup | `hyperfine --warmup 5 'glm-acp --version'` | same |
| ACP startup/init | driver timestamps spawn→first valid initialize response; cron off; no custom MCP, then slow MCP | separates process/startup/SDK |
| Idle memory | initialized process, no sessions, 60s RSS/PSS sampling | allocator/runtime cost |
| Active streaming | local server sends 10MB/100K tiny deltas, reasoning/content/tools | network fixed; peak RSS/event latency |
| Session save/load | schema-1 fixtures at 10/1K/10K messages; fsync on temp filesystem | JSON codec vs I/O |
| SQLite search | 1K/10K sessions; cold/warm FTS queries and rebuild | same DB fixture/query semantics |
| Large replay | ACP sink consumes 10K messages; time/event count/RSS | serialization/backpressure |
| Repository traversal | 100K fixture with gitignore/symlinks; rg available/absent | external rg and fallback reported separately |
| Grep/file search | hit/no-hit/500-hit/huge binary fixtures | tool overhead vs rg child time |
| Patch | 1KB/1MB, 1/100 hunks; patch-set 2/100 files | parse/validate/write/fsync separately |
| Checkpoint | 10K files 1GB logical, repeated dedup | scan/hash/compress/write and bytes stored |
| Rollback | no-conflict/conflict/100 files/injected failure | preflight and writes separately |
| Command startup | no-op command 1K runs | spawn/shell/sandbox backend separated |
| Command cancel/tree | child+grandchild holds pipes; cancel at 10/100/1000ms | kill-to-reap latency, survivors=0 |
| TUI input | reducer enqueue→state update; PTY key→render | event and render separated |
| TUI render | Ratatui TestBackend fixed 80×24/160×50 with 1K/10K messages | no terminal I/O for reducer benchmark |
| Worker concurrency | 1/3 workers, fixed local provider delays and budgets | throughput, fairness, peak RSS |
| MCP | stdio startup/call/restart and HTTP call/recovery | SDK/transport separately |
| Packaging | wheel/PyInstaller archive/extracted RSS vs Rust compressed/executable | include optional voice assets separately |

Suggested tools: `/usr/bin/time`, `hyperfine`, `psutil` or `/proc` sampler for Python, Criterion/divan for Rust microbenchmarks, iai-callgrind selectively, `perf` where available, and a repository-owned end-to-end benchmark driver emitting versioned JSON.

## Instrumentation boundaries

Each trace uses spans:

- process startup;
- ACP decode/dispatch/encode;
- session lock wait/load/save/index;
- provider request build/connect/TTFB/chunk decode/callback/backpressure;
- remote elapsed (first byte to terminal) vs local parse;
- tool permission wait;
- tool queue/spawn/execution/output/cleanup/postprocess;
- compaction deterministic extraction vs auxiliary call;
- TUI reducer/render/terminal flush.

Permission wait and remote model/network latency are excluded from “harness CPU speedup” but reported.

## Baseline fixtures

1. `repo-tiny`: 100 UTF-8 files, two languages, basic Git.
2. `repo-medium`: 10K files, ignored dependencies, CRLF, binaries, symlinks, nested instructions.
3. `repo-large`: generated 100K metadata fixture, large files, Git changes/worktrees; contents generated in temp, not committed.
4. `sessions`: minimal, 1K-turn, 10K-turn, reasoning-heavy, tool-heavy, corrupt/legacy.
5. `stream`: tiny deltas, large deltas, interleaved tools, incomplete/retry/cancel.
6. `process-tree`: cross-platform parent/grandchild/pipe holder.
7. `checkpoint`: repeated/unique blobs, secrets/ignored files, conflict states.

Each generator has a seed and manifest hash.

## Acceptance criteria

- No local median regression over 10% or P95 regression over 20% without explained tradeoff.
- Startup and idle RSS targets are set only after measuring Python on CI reference hardware.
- Stream parser memory is O(max line + bounded accumulators), not O(full wire body) beyond required response/session history.
- Cancellation leaves zero descendants and reaches terminal state within the platform-defined budget.
- TUI input-to-reducer P95 <16ms and render P95 <33ms on reference fixture are initial goals, not source claims.
- Artifact sizes remain inside existing CI ceilings (40 MiB onefile smoke context; 200 MiB compressed voice bundle) until new packaging policy is approved.

## Reporting

Versioned JSON includes Git commits, toolchains, dependency locks, hardware/OS, fixture hashes, environment flags, raw samples and excluded outliers with reasons. Markdown is generated, never hand-edited. Compare Python/Rust flamegraphs only after a statistically material difference.

## Known baseline evidence

Source documentation reports a prior `--version` improvement from roughly 0.82s to 0.16s after lazy imports, but this is not reproduced in this mission and must not be used as the Rust baseline. CI enforces artifact ceilings (`.github/workflows/ci.yml`, `.github/workflows/release.yml`).

## Completion status

All requested measurements have controlled methods, fixtures, isolation boundaries, comparison rules, and provisional acceptance criteria. No claim of Rust speedup is made.
