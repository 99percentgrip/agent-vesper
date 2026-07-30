# Stage 4 fixture coverage

Status: COMPLETE

The authoritative corpus remains 76 scenarios and 154 indexed payloads with
SHA-256 `d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a`.
No fixture payload changed.

`fixtures/coverage-stage4.json` classifies all 76 scenarios. Twelve ACP scenarios
are assigned Stage 4 adapter/runtime implementation evidence. Three existing
GLM scenarios now also cite real-process Stage 4.1 evidence for retry,
continuation, and visible-output interruption. The seven process vectors are
listed separately because concurrency and backpressure are transcript tests,
not new canonical oracle scenarios. Persistence, tools, policy execution,
process tools, and security runtimes retain explicit future owners.

`cargo xtask fixtures coverage --stage 4` validates scenario completeness,
ownership, and Stage 4 test references without adding the coverage file to the
authoritative fixture hash index.
