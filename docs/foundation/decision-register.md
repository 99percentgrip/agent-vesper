# Stage 0 Decision Register

Status: COMPLETE

## Objective

Freeze parity behavior where the mission already decides it, recommend safe
defaults where it does not, and identify choices that still require product
approval before production crate contracts are fixed.

## Methods and evidence

The register applies the frozen behavioral/persistence/security contracts,
inspects the source release matrix (`registry/agent.json:14-34`,
`.github/workflows/{ci,release}.yml`), and uses current primary package
metadata. Local tools are Rust/Cargo 1.95.0. `cargo info` confirms
`agent-client-protocol` 2.0.0 requires Rust 1.88.0. Context7 initially exposed
the Rust SDK repository main branch as 1.2.0, while crates.io exposes 2.0.0;
therefore dependency resolution uses the published crate and records wire
protocol separately. `rusqlite` 0.40.1 uses edition 2024 and its upstream
policy follows current stable Rust.

## Decisions

| ID | Decision | Status | Contract now | Approval still needed |
|---|---|---|---|---|
| A | State location | RECOMMENDED | New Vesper-owned state root; legacy `.glm-acp` is read-only until explicit import/migration; no silent move, overwrite, dual-write, or cleanup | Exact platform directory name and migration UX |
| B | Reasoning persistence | APPROVED BY EXISTING REQUIREMENT | Parity fixtures preserve Python default-on and import both present/absent reasoning; opaque provider blocks stay provider-namespaced | Any new-store default change and retention/export UX |
| C | Bypass semantics | APPROVED BY EXISTING REQUIREMENT | Policy denial remains absolute; initial parity keeps Bypass only after policy | Any later strengthening beyond source behavior |
| D | Plan Mode MCP | APPROVED BY EXISTING REQUIREMENT | Generic MCP allowance is captured exactly for parity | Restricting or removing the allowance |
| E | Command execution | APPROVED BY EXISTING REQUIREMENT | New architecture separates argv-native execution from explicitly named shell-string execution; legacy `run_command` behavior is a compatibility surface with destructive permission and process-tree ownership | Deprecation/removal schedule for shell compatibility |
| F | TUI compatibility | RECOMMENDED | Preserve every slash command, palette action, configurable binding, and accessibility operation; accessible behavioral equivalence, not pixel identity | Product acceptance of layout changes and any retired operation |
| G | Rust/MSRV/targets | RECOMMENDED | MSRV 1.88.0, current stable also tested; five source release families remain validation/release targets: Linux x86-64/aarch64, macOS x86-64/aarch64, Windows x86-64 | Long-term MSRV cadence and whether any target may become best-effort |
| H | Repository initialization | APPROVED BY EXISTING REQUIREMENT | Initialize target Git repository on `main`, preserve files, no remote, no commit | None |

## Compatibility boundary

- “Approved by existing requirement” means the mission text fixes initial
  parity behavior; it is not invented product approval for a later change.
- Recommendations are safe Stage 1 assumptions only if contract surfaces
  remain configurable or isolated. Production persistence, TUI, and release
  contracts must not hard-code the pending choices.
- Initial fixture comparison intentionally preserves current Bypass and Plan
  Mode behavior even if a later security ADR changes it.

## Commands

- Current docs research through Context7 for ACP, rusqlite, and reqwest.
- `cargo search agent-client-protocol --limit 5`
- `cargo info agent-client-protocol`
- `cargo info rusqlite`
- `cargo info reqwest`
- `rustup show active-toolchain`
- `rustup target list --installed`
- release-target search across source workflows and `registry/agent.json`
- `git init -b main`; branch/status verification

## Files created

This register and ADRs 0001–0008 under `docs/foundation/adr/`.

## Test/result classification

- Repository initialization: **locally validated** (`main`, no commit/remote).
- Package/MSRV facts: **confirmed current package metadata**.
- Five-target behavior: **CI validation pending**; only Linux x86-64 is local.
- Items A, F, and the pending columns above: **product decision pending**.

## Migration readiness

No pending product choice prevents fixture capture or disposable spikes.
Production state-writing, TUI, and release contracts must remain gated.

