# Stage 3 Fixture Coverage

Status: COMPLETE

The authoritative corpus remains unchanged at 76 scenarios and 154 indexed
payloads. Its fixture-index SHA-256 remains
`d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a`.

`fixtures/coverage-stage3.json` covers every scenario. All 21 `provider/glm`
source scenarios are implemented and exercised by the production adapter.
Applicable contract vectors for redaction, fallback visibility, fragmented
parallel tools, opaque reasoning, terminal uniqueness, unknown finishes, and
usage provenance are represented by adapter/unit plus Stage 2 conformance
tests. The remaining 51 scenarios retain explicit future ownership; ACP,
sessions, tools/process, policy integration, and unrelated security runtimes
are not claimed.

The adapter contributes 25 focused tests; the whole workspace has 88 tests.

Run `cargo xtask fixtures coverage --stage 3` to regenerate and validate the
map and `cargo xtask provider glm verify` for adapter conformance.
