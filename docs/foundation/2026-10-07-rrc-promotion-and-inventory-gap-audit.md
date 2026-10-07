# RRC promotion and Git inventory gap audit

## Objective and status

The later [authorization incident repair](2026-10-07-rrc-authorization-stall-diagnosis.md) changes source again. The archived verification below certifies its frozen source only, not that later patch.

Alex requested another search for gaps after the v0.24.7 release and fresh PRD reverification. Audit uncovered failure paths, reproduce confirmed defects, fix them in the provider-neutral controller, and verify the changed source without replacing historical evidence.

Worktree: `/tmp/vesper-rrc-closeout-fix`, branch `repair/rrc-automatic-closeout`. Base and immutable v0.24.7 source: `0857f02b8dbdc2a867cdb93bf86a03559fb7555b`. All five reproduced defects are fixed and verified locally. Current repairs are uncommitted and unreleased; the final receipts below bind their exact source bytes.

## Methods and confirmed defects

1. **Proof and promoted source could diverge.** The repair executor staged source, reran focused/full gates, then collected a new diff. A verifier that passed the real regression and subsequently changed `answer()` from 42 to 43 produced a failed final regression but was promoted as a passed repair. The red regression observed 43 in the controller workspace. Pin complete status, full HEAD patch and index tree before native proof; refuse changes after every focused command and the full verification pass, recording a failed repair and preserving admission limits. Freeze the promotable delta before proof and apply those exact bytes.
2. **Git paths were interpreted as display text.** Line slicing left Git's quoted paths quoted, split embedded newlines, and default status collapsed a new directory to `docs/`. Legitimate admitted repairs were refused. NUL-delimited porcelain with all untracked files preserves literal UTF-8 paths; cached path inventories disable rename collapsing and retain both paths. Incomplete/non-UTF-8 inventories fail closed. A separate red regression showed staged rename sources failed a second `git add` after their removal. Stage only admitted existing or still-indexed paths; already-staged deletions/rename sources remain in the index without another add.
3. **Display truncation hid an unadmitted staged file.** `checked()` redacts/truncates stdout to 4096 characters for display. Admission reused that receipt as its file inventory. With 256 admitted sixteen-byte status lines, `zzz-unadmitted.txt` fell beyond the receipt and was committed. Machine inventories now use raw bounded Git output, independently of display redaction/truncation; hitting the 4 MiB subprocess bound refuses admission.

4. **Ignored new source could supply nonreproducible proof.** An additional red regression wrote Git-ignored `src/generated.rs`, repaired `src/lib.rs` to import it and passed real tests in the repair worktree. Promotion included the importer but omitted its implementation. Every surviving file successfully written by the bounded repair tools must now be represented in the staged tree before native proof. This includes single-file tools and patch sets; ignored files already tracked remain eligible. Failed verification also retains bounded redacted cause text for the next attempt's untrusted context, preserving its admission/budget counters.

5. **The whitespace check inspected an empty unstaged diff.** After `git add --all`, native `git diff --check` did not inspect the staged repair. A new red regression promoted a Markdown file with trailing whitespace. Check the cached delta against the captured baseline tree within the native verification transaction; a failure now records failed proof and its safe cause without promotion.

All fixes live in shared `vesper-harness::release_executor`; TUI and ACP use that same implementation. No provider-name branch, adapter dependency, host-local release logic or retry-budget reset was introduced.

## Inspected and changed files

- `crates/vesper-harness/src/release_executor.rs`: version preparation, candidate admission/staging, bounded repair proof/promotion, complete Git parsing and regressions. Corrected a stale backoff comment to the existing 20/40/80/120-second behavior.
- `crates/vesper-harness/src/release_recovery.rs`: admission tokens, repair receipts/budgets and failure transitions; unchanged.
- `crates/vesper-harness/AGENTS.md`: proof immutability, complete literal inventories and deletion/rename staging contracts.
- Owning RRC PRD section 37, foundation report/index/ownership: link new findings and preserve the old exact-source boundaries.

## Verification receipts

[Machine receipt](2026-10-07-rrc-promotion-and-inventory-gap-audit-evidence.json) and [raw receipt archive](2026-10-07-rrc-promotion-and-inventory-gap-audit-receipts.tar.gz) bind the final source and actual results. Final Rust source SHA-256: `bd3edeeb780ac177474e6a5d47eb0bc4e4d063b6266f6d266dc07887689ae10a`. Archive SHA-256: `02b6f700f77886f2042ceea7bd0128f2e9c177f4807e73335333a490bc5bde5c`.

Final sequential checks used `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1` and the existing shared test target. Every command exited 0:

| Command | Observed result |
| --- | --- |
| `cargo test --locked -p vesper-harness --lib --all-features release_executor::tests:: -- --nocapture` | 81 passed, 0 failed; 113.53 s |
| `cargo check --locked --workspace --all-targets` | Default-feature workspace check passed |
| `cargo xtask verify` | Formatting, all-target/all-feature Clippy with warnings denied, workspace tests, remaining canonical checks and embedded acceptance passed |
| Harness library within workspace verification | 383 passed, 0 failed, 6 ignored; 258.03 s |
| `cargo xtask acceptance` | 127/127 exact cases passed; 123407 ms; offline model cost zero |
| `cargo xtask architecture` | 31 packages validated |
| `cargo +1.88.0 check --locked --workspace --all-targets --all-features` | Rust 1.88 locked all-feature/all-target workspace check passed; 3m 24s |

All **100 unique PRD-mapped cases** from the earlier 36-section/23-criterion trace were observed passing again against this source. All **eight new regression cases** passed in the full workspace run. None of those 108 named cases was ignored or missing. This extends coverage; it does not turn passing tests into proof of universal bug absence.

The composed positive/negative cases execute real source tools and native Rust commands for both fixture provider identities and both repair routes. Verifier changes cover tracked source, staged index and new untracked files. New literal paths have a positive promotion case; ignored imported source and staged Markdown whitespace have negative cases. Failed receipts preserve their safe cause, consume only the admitted repair count, retain `source_commit_after: None`, and spend no full-gate retry.

Red evidence:

- `/tmp/vesper-rrc-patch-stability-red.log`: one failed regression; previously promoted value 43 rather than preserving the original broken controller source.
- `/tmp/vesper-rrc-path-admission-red.log`: three failed regressions against the old native candidate method, with current tests retained: unowned staged tail admitted, untracked directory collapsed, staged quoted filename refused.
- `/tmp/vesper-rrc-rename-red.log`: staged rename source pathspec failure after literal inventory repair and before staging repair.
- `/tmp/vesper-rrc-ignored-source-red.log`: one failed regression; the importer reached the controller without the ignored implementation after real passing native proof.
- `/tmp/vesper-rrc-staged-whitespace-red.log`: one failed regression; a staged Markdown file with trailing whitespace was promoted despite the old diff check.
- An initial green-run fixture lacked `Cargo.lock`; its fixture setup was corrected. That intermediate run was 74 passed/1 failed and is retained as `/tmp/vesper-rrc-deeper-audit-focused.log`, not counted as a green gate.

Intermediate verification deviations:

- `/tmp/vesper-rrc-deeper-audit-focused-green.log` is a **failed intermediate run** despite its filename: 77 passed, 1 failed after 451.32 seconds. A Cargo fixture waited on the concurrently started workspace build lock and reached its unchanged 300-second command inactivity watchdog. Process inspection observed `locks_lock_inode_wait`; the fixture failure was retained, and final checks run sequentially without increasing that bound.
- Two broad verification runs passed formatting/Clippy and began compiling or running workspace tests, then were explicitly interrupted before incorporating the newly discovered ignored-source and staged-whitespace corrections. Their logs are retained as superseded evidence, not final green gates. No watchdog or test gate was weakened.
- The prior four-gap source passed all 80 focused controller tests in 101.85 seconds; final verification below additionally covers the staged-whitespace correction.

## Scope and unresolved boundaries

Real isolated Git worktrees, file tools, and failing/passing Rust commands prove controller behavior under two synthetic provider identities, for both local preparation and remote repair routes. The test-only full-gate port is substituted to inject verifier changes; production still executes all native gates. These fixtures do not prove live model repair effectiveness.

No live provider call, production release operation, installed-app change, tag mutation or new hosted matrix was performed in this audit. Historical v0.24.7 green matrices cannot certify the changed source. Fresh hosted platform/publication gates remain unexecuted. This bounded audit fixes confirmed defects; it cannot establish absence of every possible bug.

## DOX pass and readiness effect

Read root, crates/harness and docs/foundation contracts. Updated nearest behavior/ownership contracts and PRD/index links. Root, `crates/AGENTS.md` and `docs/AGENTS.md` remain unchanged because shared ownership, dependencies, hierarchy and release policy did not change; all Child DOX Index entries remain valid. Report and machine receipts are local until an authorized code candidate is committed/pushed with its documentation.

Readiness: all five confirmed defects in this pass are locally repaired and verified. The prior passing inventory missed these cases, so it cannot justify a blanket all-gaps-fixed claim. Fresh exact-commit hosted and producing gates remain required before publishing this changed source.
