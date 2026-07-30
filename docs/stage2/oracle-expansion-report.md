# Stage 2 Oracle Expansion Report

Status: COMPLETE

## Objective

Fill genuine provider-neutral evidence gaps without modifying or regenerating
the 65 frozen-source scenarios.

## Method

The 53 deferred Stage 1 scenarios were audited first in
`fixtures/coverage-stage2-plan.json`. Eleven semantics could not be captured
from a single-provider Python implementation, so
`tools/python-oracle/generate_contract_fixtures.py` generates deterministic,
language-neutral vectors under `fixtures/contracts/`.

Every synthetic manifest contains
`input.synthetic_future_contract=true`; every result uses runner
`contract-generator`, classification `locally-validated`, zero network/process
activity, and the frozen source commit only as the migration evidence anchor.
They are not represented as reproduced Python behavior.

## Added scenarios

- ACP message-ID linkage and command/event correlation;
- provider error redaction;
- observable fallback;
- fragmented parallel tool-call identity;
- invalid legacy-session bound;
- opaque reasoning continuation;
- terminal uniqueness;
- unknown extension round trip;
- unknown finish-reason preservation;
- delta/cumulative usage provenance.

## Determinism evidence

Two independent generations under separate `/tmp/vesper-contract-*` roots
produced the same logical generator hash:

```text
d51f733180d2ae3c6d76f42827656e412936590cde1cbe27a6e5e9e5460fd8da
```

`diff -ru` produced no differences. The actual corpus validates as 76 scenarios
and 154 indexed payloads. Final index-file SHA-256:

```text
d09edfe2169df49e0cfef9a66083a7df046651f441deb0e78bc0c855dec6db7a
```

Coverage planning/generated maps are intentionally excluded from the
authoritative payload index. The two JSON schemas are indexed.

## Schema changes

Manifest category `contracts` and result runner `contract-generator` were added
to the version-1 schemas as backward-compatible enum extensions. The Python
coordinator now validates all discovered scenario pairs and has an explicit
`rebuild-index` operation. Source capture logic and expected source behavior
were not changed.

## Commands

```text
python3 tools/python-oracle/generate_contract_fixtures.py --output <tmp-a>
python3 tools/python-oracle/generate_contract_fixtures.py --output <tmp-b>
diff -ru <tmp-a> <tmp-b>
python3 tools/python-oracle/generate_contract_fixtures.py --output fixtures/contracts
python3 tools/python-oracle/oracle.py rebuild-index
```

## Unresolved

No new live/provider evidence was sought. Exact GLM wire semantics remain the 21
source-derived fixtures and belong to Stage 3.

