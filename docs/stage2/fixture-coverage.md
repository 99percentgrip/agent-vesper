# Stage 2 Fixture Coverage

Status: COMPLETE

## Corpus

| Class | Scenarios | Meaning |
| --- | ---: | --- |
| Frozen-source fixtures | 65 | Reproduced or confirmed against source commit |
| Synthetic future-contract vectors | 11 | Provider-neutral requirement absent from source |
| Total | 76 | All parsed and schema validated |
| Indexed payloads | 154 | 152 manifest/result files plus two schemas |

Category counts: ACP 12, provider/GLM 21, sessions 7, tools 10, policy 6,
security 5, process 4, contracts 11.

## Stage 2 result

All 76 scenarios have at least one implemented contract representation and
test reference. Fifty-three still contain real runtime behavior deferred to a
precise owner; 23 have no remaining Stage 2-relevant runtime behavior. This does
not mean 76 runtime behaviors are implemented.

Examples:

- GLM SSE scenarios implement request/stream/error/terminal expressiveness but
  defer byte parsing and transport to Stage 3.
- ACP scenarios implement command/event/identity/order expressiveness but defer
  SDK/wire dispatch.
- session scenarios implement the read/write-free codec but defer persistence.
- tool/process scenarios implement schemas, linkage, policy class, and bounded
  observations while deferring execution.

## Machine-readable records

- `fixtures/coverage-stage2-plan.json`: audit of the original 53 deferred
  scenarios, evidence strength, contract/runtime surfaces, and future owner.
- `fixtures/coverage-stage2.json`: all 76 scenarios, implemented contracts,
  deferred runtime behavior, tests, source/synthetic classification, and exact
  fixture-index hash.

`cargo xtask fixtures coverage --stage 2` regenerates the final map, and
`cargo xtask contracts verify` rejects missing scenarios, empty implemented
contracts, missing future owners, or an incorrect synthetic-vector count.

## Negative fixture

`tool.command-cancellation` retains the confirmed Python descendant leak as a
negative source fixture. Stage 2 represents cancellation/process observations;
the future Rust process stage must pass the strengthened no-descendant-leak
criterion rather than reproduce the defect.

