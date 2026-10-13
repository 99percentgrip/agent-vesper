# quinn-proto security remediation

## Objective

Clear the five RustSec advisories that failed PR #42 supply-chain validation, without unrelated dependency upgrades.

## Change

`reqwest 0.13.4` pulls `quinn 0.11.11`, which pulls `quinn-proto`. `cargo update -p quinn-proto --precise 0.11.18` moved only that crate from 0.11.16 to 0.11.18. Its published Rust version is 1.85, inside the workspace MSRV of 1.88. The resolver also selected the already-present `getrandom 0.3.4` for `tempfile` instead of `getrandom 0.4.3`. No other package version changed. No advisory was ignored.

## Evidence

- `cargo audit` 0.22.2 exited 0. The scan reported no vulnerabilities.
- `cargo deny --all-features check` 0.20.2 exited 0. Duplicate-crate warnings are pre-existing and did not fail the command.
- `cargo xtask verify` exited 0. The acceptance gate reported 165 exact cases passed. Log: `/tmp/quinn-xtask-verify.log`.

The five advisories, RUSTSEC-2026-0338, 0339, 0340, 0341, and 0342, require `quinn-proto >= 0.11.18`.
