# Python Oracle

## Purpose

Capture canonical fixtures from the frozen Native GLM ACP source.

## Ownership

- `oracle.py` validates schemas, runs isolated workers, canonicalizes results,
  and owns the fixture hash index.
- `source_worker.py` imports the frozen source only inside an isolated child
  process and executes deterministic probes.

## Local Contracts

- Require an explicit source path and exact frozen commit.
- Use the source `.venv` only to import source dependencies; never install into
  or write inside it.
- Redirect HOME/config/cache/pycache/tmp and disable cron/live provider use.
- Kill the complete worker process group on timeout.
- Treat source stdout/stderr, exceptions, paths, and headers as untrusted;
  sanitize before persistence.

## Work Guidance

- Scenario outputs are source observations, not hand-authored expectations.
- Local HTTP servers bind loopback only and expose no credential values.

## Verification

- `python3 tools/python-oracle/oracle.py validate-all`
- `python3 tools/python-oracle/oracle.py verify-index`
- Two `capture-all` runs must produce the same hash index.

## Child DOX Index

No children.
