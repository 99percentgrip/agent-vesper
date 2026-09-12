---
name: work-unit-reporting
description: "Complete every work unit with BOTH a formal .md execution report in the workspace evidence directory AND a full summary in the chat; run regression-first final audits on multi-part initiatives."
version: 1.0.0
author: Agent Vesper library
license: MIT
platforms: [linux, macos, windows]
tags: [reporting, evidence, audit, quality, completion]
related_skills: [verify-with-xtask-verify, harness-skill-gate-mechanics]
---

# Work Unit Reporting & Final Audit

Two artifacts close every work unit — a file the workspace keeps, and a
summary the human reads. Neither alone is completion.

## When to Use

- Any completed implementation, repair, reconnaissance, audit, or
  multi-step directive.
- Multi-part initiatives: per-unit reports while executing, plus one final
  audit when the initiative completes (or on explicit request).

## Procedure

1. **Formal report (.md)** in the workspace's evidence directory (follow
   the local convention — e.g. `docs/foundation/`, an `evidence-index.md`
   entry, or the nearest owning doc). Structure:
   objective · methods/commands · files changed · exact evidence with
   verbatim command receipts · deviations from instructions · unresolved
   items · readiness effect. Link it from the evidence index and the
   owning requirements document.
2. **Full summary in the chat, same delivery turn** — status, what was
   built, constraints held, verification receipts, deviations, open
   items, next step. A file link alone is NOT a summary.
3. **Honesty rules:** preserve missing/failed/stale/unexecuted acceptance
   items instead of weakening scope; every completion claim traces to
   current, scope-appropriate evidence (passing unrelated tests, plan
   checkmarks, and another agent's assurance are not evidence); record
   deviations explicitly.
4. **Release-sensitive code** (paths, canonicalize, shell, raw bytes):
   follow the workspace's verification gates — local verification first,
   then the cross-platform CI matrix before claiming done.

## Final Audit (initiative completion or explicit request)

1. Re-read every requirement document and execution report; re-trace each
   acceptance claim to current code — not to the report that claims it.
2. Re-derive invariants by hand (budgets, fail-closed paths, parity).
   Distrust duplicated logic: two layers with one rule drift apart.
3. Check each self-test for vacuity: an invariant test can encode the bug
   it guards (assert the definition, not the implementation's output).
4. Re-read every narrative claim against its own recorded table — reports
   overstate exactly where nobody re-checks.
5. **Regression first:** for each finding, write a test that FAILS on the
   pre-fix code, confirm the failure, then fix. A test that never failed
   proves nothing.
6. Correct overstated evidence in place with an explicit audit note; never
   silently rewrite history.
7. Re-derive verdicts from corrected data; if a decision no longer holds,
   revert it and say so.

## Pitfalls

- Summarizing in chat but never writing the durable file (or the reverse).
- Tests that pass both before and after a "fix" — vacuous; sabotage-check
  them by reverting the fix mentally or actually.
- Publishing measured tables before re-reading the narrative against them.
- Treating deliberate open items (CI pending, evidence-barred deferrals) as
  unfixed bugs, or hiding real bugs as "deferred by decision" — name which
  is which, with justification.

## Verification

After writing a report: confirm cited paths exist, re-run the quoted gate
commands to confirm receipts are current, and check the evidence-index
link resolves. After an audit: confirm every regression test fails on the
pre-fix tree's behavior description and passes on the current tree.
