# Completion-Reporting Workflow — Execution Report (Productization)

Status: **COMPLETE — locally verified** (2026-09-12)
Scope: turn the session's reporting/audit conventions into shipped
behavior of every installed Agent Vesper — mandate + methodology + seed —
per Alex's directive ("report after each completion, .md and in the chat;
release pipeline; final audit").

## 1. Objective

A user who installs Agent Vesper gets, by default: both-artifact
completion reporting (formal .md report + full in-chat summary), honest
evidence rules, regression-first final audits, and release-gate
discipline — without workspace-specific configuration.

## 2. Design (three layers)

1. **Binding mandate (system prompt):**
   `vesper_harness::COMPLETION_REPORTING_INSTRUCTION` — one shared,
   bounded, cache-stable constant (precedent:
   `COGNITIVE_CAPABILITY_INSTRUCTION`), injected by BOTH hosts at every
   agent-loop build: ACP interactive path + ACP worker path, TUI direct
   path + TUI tool-loop path. Mandates: file report + in-chat summary
   ("Neither alone completes a unit; a file link is not a summary"),
   honest-evidence rules, final-audit discipline, release gates.
2. **Methodology (seed skill):** `work-unit-reporting` v1.0.0 — procedure,
   final-audit checklist (re-trace claims to code, re-derive invariants,
   vacuity-check self-tests, re-read narrative against tables,
   regression-first fixes, in-place audit corrections, re-derive
   verdicts), pitfalls, and verification steps. Registered in the
   `software-development` bundle (22→23) so `bundle:<name>` routing finds
   it; library 94→95; mirrored to the global home; seeded into every
   fresh install non-destructively per `scripts/AGENTS.md` (existing
   files win, deletions respected).
3. **Release pipeline (existing, composed):** the exact-commit release
   contract (push version commit → canonical/MSRV/five-target/web-driver
   workflows green on that commit → immutable tag) stays authoritative;
   the new skill composes with `verify-with-xtask-verify` (whose five
   `gh run watch` targets were re-verified against
   `.github/workflows/platform-foundation.yml` + `release.yml`) rather
   than duplicating it.

## 3. Methods and command record

| Gate | Command | Result (receipt) |
|---|---|---|
| Shared const builds | `cargo build -p vesper-harness` | `Finished … 9.84s` |
| ACP parity test | `cargo test -p agent-vesper-acp` | `tests::completion_reporting_mandate_is_injected_and_matches_shared_contract … ok`; suite `52 passed; 0 failed` |
| TUI parity test | `cargo test -p agent-vesper-tui completion_reporting` | header + verbatim-mandate assertions `1 passed` (extended inside the existing prompt-composition test, cognition on/off) |
| Lint | `cargo clippy -p vesper-harness -p agent-vesper-acp -p agent-vesper-tui --all-targets --all-features -- -D warnings` | clean |
| Format | `cargo fmt --check` (three crates) | clean |
| Workspace | `cargo test --workspace --all-features` | `TOTAL: 2218 passed, 0 failed` |
| Acceptance | `cargo xtask acceptance` | `20 exact cases passed in 18825 ms` |
| Architecture | `cargo xtask architecture` | `27 packages` |
| Naming | `cargo xtask naming-guard` | `clean (18 hits, all frozen in baseline)` |
| Seed count gate | `find skills/skills -maxdepth 1 -name '*.md' \| wc -l` vs bundle refs | `95` == `95` |
| Seed mirror | `cp` + compare | mirrored to `~/.agent-vesper/memory/skills/` |

## 4. Files changed

`crates/vesper-harness/src/lib.rs` (const),
`apps/agent-vesper-acp/src/lib.rs` (helper + 2 injection sites + parity
test), `apps/agent-vesper-tui/src/main.rs` (helper + 1 injection site +
extended prompt-composition test), `skills/skills/work-unit-reporting.md`
(new seed, mirrored), `skills/bundles/software-development.json` (22→23),
`skills/AGENTS.md` (count 94→95 + ownership note), `scripts/AGENTS.md`
(seed contract note), `crates/AGENTS.md` (harness child-index note),
`AGENTS.md` (productization note on the existing preference),
`docs/migration-status.md` (new row).

## 5. Parity proof detail

Both hosts assert the identical shared constant: the ACP test asserts the
injected block equals `vesper_harness::COMPLETION_REPORTING_INSTRUCTION`
byte-for-byte plus the verbatim mandate lines; the TUI test asserts the
header and the "Neither alone completes a unit" line inside the real
prompt composition (cognition on and off, matching its existing
enforcement-test pattern). This satisfies the repo's bidirectional
host-parity contract: a host-agnostic behavior change wired into both
hosts in the same change, with registration/advertisement tests.

## 6. Deviations from the directive

1. **Mandate lives in `vesper-harness`, not a new workflow file** — the
   release workflows already exist and ship; what was missing was the
   behavioral mandate + methodology for installed agents. No new CI
   workflow was authored (the exact-commit contract already covers
   release gating); the skill composes with the existing
   `verify-with-xtask-verify` rather than duplicating it.
2. **The evidence directory is convention-resolved, not hardcoded** —
   installed agents work in arbitrary workspaces; the mandate says
   "the workspace's evidence directory (follow the local convention)"
   and the skill teaches the structure. Vesper's own convention
   (`docs/foundation/` + `evidence-index.md`) remains the reference
   implementation.
3. **No new slash command or settings surface** — the directive asked for
   behavior, not UI; per the provider-neutral contract, a settings toggle
   would be the vehicle if one is ever wanted (not built speculatively).

## 7. Unresolved items (honest scope)

- **Effectiveness is behavioral, not yet measured:** the mandate changes
  model-facing instructions; no eval measures report compliance of
  installed agents. The acceptance framework could enroll this as an
  objective later.
- **CI:** local verification only; the exact-commit matrix gates the
  eventual release, not this local completion claim.
- **Seed upgrade path:** existing installs gain the skill on next
  upgrade (non-destructive seeding); installs that explicitly deleted it
  keep their deletion (correct by design).

## 8. Readiness effect

Installed agents now carry the reporting/audit contract by default:
file report + in-chat summary per work unit, honest evidence, final
audits with regression-first proofs, release-gate discipline. This
workspace's convention and the product's shipped behavior are the same
rule, expressed once in the shared mandate.
