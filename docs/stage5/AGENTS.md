# Stage 5 records

## Purpose

Own evidence and scope records for staged read-only persistence, conversion,
identity, replay, runtime injection, ACP lifecycle integration, security, disk
invariance, fixture/testkit governance, and Stage 6 readiness.

## Ownership

- `evidence-index.md` is the durable command, evidence, change, and verification
  ledger for staged read-only persistence work.
- `disk-invariance-proof.md` records per-vector process-path hash, file-set,
  timestamp, state-directory, redaction, and concurrency evidence.
- `session-store-report.md` is the final read-only store and verification
  summary.
- `legacy-discovery.md` records confirmed roots, filename policy, enumeration,
  and sidecar behavior.
- `runtime-load-and-resume.md` records repository injection, collision,
  keyed-load, adoption, and lifecycle semantics.
- `replay-contract.md` records visible-history filtering, deterministic
  identity, update order, and writer acknowledgement.
- `stage6-readiness.md` defines the bounded transactional-write handoff without
  implementing it.

## Local Contracts

- Separate implemented read-only discovery/decoding/conversion/replay/runtime
  injection, process proof, and governance from deferred transactional writes.
- Record source behavior with file and symbol evidence.
- Do not claim missing remote platform validation as complete.

## Work Guidance

- Keep commands and results current as the stage progresses.

## Verification

- Run `cargo xtask sessions verify`, `cargo xtask architecture`, and the full
  workspace/MSRV/supply-chain suite.

## Child DOX Index
