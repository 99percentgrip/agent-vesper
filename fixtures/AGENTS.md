# Differential Fixtures

## Purpose

Own deterministic, language-neutral Python-to-Rust compatibility scenarios and
captured results.

## Ownership

- `schema/` owns the versioned manifest and result-envelope JSON Schemas.
- Category directories own scenario manifests, inputs, and canonical results.
- `manifest-sha256.json` indexes all fixture files by content hash.
- `coverage-stage1.json` records parsed, schema-validated, implemented, and
  deferred Stage 1 coverage without changing the oracle hash index.
- `coverage-stage2-plan.json` audits contract/runtime ownership before Stage 2
  implementation.
- `coverage-stage2.json` records executable Stage 2 contract coverage and exact
  future runtime ownership.
- `coverage-stage3.json` records GLM adapter implementation and exact ownership
  for all remaining runtime scenarios.
- `coverage-stage4.json` records ACP adapter and ephemeral-runtime coverage
  plus Stage 4.1 process-vector evidence without entering the authoritative
  fixture hash index.
- `coverage-stage5.json` records source-versus-synthetic provenance, read-only
  session/runtime/replay/process evidence, explicit future owners, and zero
  persistent writes without entering the authoritative fixture hash index.

## Local Contracts

- Every scenario cites source commit and comparison class.
- Fixtures contain no credentials, private user state, or live-provider text.
- Tracked JSON fixtures use LF line endings so byte-level hashes remain stable
  across checkout platforms.
- Normalize only declared volatility; never normalize order, linkage, policy
  outcome, finish reason, hashes, redaction, or containment results.
- Generated results must validate against `schema/result-v1.schema.json`.

## Work Guidance

- Prefer source capture over hand-authored expected behavior.
- Use deterministic local services, seeds, and temporary workspace roots.

## Verification

- Run `tools/python-oracle/oracle.py validate-all`.
- Run `tools/python-oracle/oracle.py verify-index`.
- Run `cargo xtask fixtures validate` and `cargo xtask fixtures verify-index`.
- Run `cargo xtask fixtures coverage --stage 2`.
- Run `cargo xtask fixtures coverage --stage 3`.
- Run `cargo xtask fixtures coverage --stage 4`.
- Run `cargo xtask fixtures coverage --stage 5`.
- Capture twice and compare `fixtures/manifest-sha256.json`.

## Child DOX Index

No children.
