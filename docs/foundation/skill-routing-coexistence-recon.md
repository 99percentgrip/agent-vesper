# Skill routing / score-floor coexistence reconnaissance

Date: 2026-09-13. Status: reconnaissance complete; isolated implementation started.
Owner: [Skill routing quality PRD](../skill-routing-quality-prd.md).

## Objective and method

Protect GLM's concurrent score-floor work while implementing the approved skill
routing design. Read root, crates/memory and docs/foundation contracts; inspect
`git status`, `git log`, `git show --stat 5cf5835`, score-floor PRD and PR-1 report,
`rank_chunks`, its pin/control and the exemplar G5 test. Do not infer completion
from the PR-1 report's summary alone.

## Confirmed state

- Main HEAD at inspection: `5cf5835f78d8e779443188c3490172d7736a589a`.
- GLM PR-1 adds a noise anchor and genuine-overlap control. The anchor remains
  ignored because the production chunk filter remains `score > 0`. PR-2/PR-3
  are not yet present in the inspected tree. Original G5 is still bounded to
  three chunks rather than asserting zero.
- The commit also changes `skills/skills/research-paper-writing.md`; despite the
  PR-1 report's narrower file list, that source-library change is in the commit.
  Preserve the actual committed tree, not a reconstruction from the report.
- Existing uncommitted changes are our dollar parsing repair and routing planning
  documents. They were copied into the isolated checkout, not reset or stashed.

## Ownership and collision map

| Area | GLM score floor | Routing quality | Integration rule |
| --- | --- | --- | --- |
| `skill_orchestrator.rs` | `rank_chunks`, new minimum chunk score | Explicit parsing and skill selection call sites | Same file, different behavior; merge hunks, never replace whole file |
| `src/lib.rs` | Export minimum chunk score | Export routing types/module | Preserve both exports |
| `tests/chunk_routing_eval.rs` | Pin, control, D3 acceptance | Read-only compatibility surface | Never weaken/delete/unignore GLM's pin on its behalf |
| `tests/exemplar_routing.rs` | Literal G5 restoration | Read-only compatibility surface | Preserve GLM's restored assertion and fixtures |
| `skills/` | Existing exemplar work | No writes | Preserve inventory and bytes of all library sources |
| Docs/AGENTS/evidence index | Score-floor records | Routing records | Combine entries without replacing ownership/history |
| Host Settings / new routing module | No declared score-floor scope | Routing implementation | Implement in isolated checkout |

Scope distinction: score floor must leave the skill-tier baseline unchanged for
its own acceptance. Routing quality may add an opt-in skill-tier mode under its
separate PRD; Standard preserves that baseline. Do not judge GLM's scope fence
against the entire combined diff or modify its historical acceptance claim.

## Isolation and integration

Development checkout: `/tmp/vesper-routing-quality`, branch
`feat/skill-routing-quality`, local shared clone of main at the commit above.
Its working files, Git index, branch and build target directory are independent
of GLM's original checkout. No branch switch, reset, stash, broad staging or
formatting in GLM's checkout. New implementation edits stay in this checkout.
The original checkout is read-only to this work until explicit integration.

Before merge, inspect GLM's finished commit, verify its pin/control/G5 and D3
receipts, fetch that commit locally and merge in the isolated checkout. Reject
conflicts that drop either requirement. Verify Standard skill scoring against
its pre-enhancement baseline and all chunk controls against GLM's final tree.
Only integrate after the original checkout is quiescent; do not move its branch
under an active process. No release or installed binary replacement is implied.

## Verification / artifacts

Five GLM-owned path hashes captured in `/tmp/vesper-routing-glm-baseline.json`.
All 488 files under the isolated `skills/` tree are recorded in
`/tmp/routing-library-before.json`; later preservation checks compare actual bytes.
Targeted `cargo test -p vesper-memory --test chunk_routing_eval
noise_floor_control_genuine_overlap_still_routes --offline` passed (1/0).
Explicit `noise_floor_pin_zero_overlap_prompt_routes_nothing -- --ignored` failed
(0/1) with `bytecode-lexicon`, matching GLM’s recorded defect. Logs are
`/tmp/routing-coexist-control.log` and `/tmp/routing-coexist-pin.log`. This expected
failure proves the pending gate was not accidentally implemented or weakened.
No live provider calls or user-state writes.

## DOX and readiness

PRD links this report and records implementation authorization and coexistence
rules. Foundation evidence index links the recon. Existing contracts cover the
report and isolated development; no new child boundary. GLM remains responsible
for score-floor PR-2/PR-3; we can implement independent routing modules and tests
now, but integrated R6 acceptance requires its finished gate.
