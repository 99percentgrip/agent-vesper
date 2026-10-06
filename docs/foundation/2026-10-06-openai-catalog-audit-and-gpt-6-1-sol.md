# OpenAI catalog audit and GPT-6.1 Sol implementation

Date: 2026-10-06

Branch: `feat/openai-gpt-6-1-sol`

Base: `56f5ce6adfb26282f50350043beec5915a9c038d` (`origin/main`)

Verdict: **IMPLEMENTED; INDEPENDENT REVIEW BLOCKERS REPAIRED WITH RED-FIRST PROOF. ACCEPTANCE, CANONICAL AND MSRV GATES PASS; EXACT-VERSION/EXACT-SHA HOSTED RELEASE CI IS PENDING. NO RELEASE ACTION.**

## Objective

Research and update Agent Vesper's native OpenAI coding-model catalog, with
`gpt-6.1-sol` as the required missing model. Audit current visible and retained
entries, preserve native API-key and ChatGPT-subscription authentication,
account-scoped discovery, credential reuse, saved selections, provider switching,
restart validation, and direct Responses transport without Codex CLI or app-server.
Both TUI and ACP must continue to derive model and reasoning choices through the
shared OpenAI provider ports.

## Methods and sources

Primary sources inspected read-only on 2026-10-06:

- [GPT-6.1 Sol model page](https://developers.openai.com/api/docs/models/gpt-6.1-sol)
- [GPT-6 Sol model page](https://developers.openai.com/api/docs/models/gpt-6-sol)
- [GPT-6 Luna model page](https://developers.openai.com/api/docs/models/gpt-6-luna)
- [OpenAI API model index](https://developers.openai.com/api/docs/models)
- [OpenAI API changelog](https://developers.openai.com/api/docs/changelog)
- [OpenAI deprecations](https://platform.openai.com/docs/deprecations)
- OpenAI Codex `codex-rs/models-manager/models.json` at commit
  [`d63a9b8344cfe58bc78bbe319b560378fc8756ef`](https://github.com/openai/codex/blob/d63a9b8344cfe58bc78bbe319b560378fc8756ef/codex-rs/models-manager/models.json),
  472,072 bytes, SHA-256
  `943ca7fe1d19ed019054158303f0dbbd80b3bef43b3aa425a71aa7cb3fb52c2b`.

The audit compared official public capability claims with OpenAI's pinned current
Codex catalog and Vesper's static capability index. Account availability remains a
separate authenticated discovery result; a static row never grants access.

## Catalog findings

| Model | Finding | Result |
|---|---|---|
| `gpt-6.1-sol` | Missing from Vesper; official page and pinned catalog agree on text/image input, text output, literal efforts `low`, `medium`, `high`, `xhigh`, `max`, API support, visible subscription row, and 128K maximum output. The public context window is 1,050,000. `none` and `minimal` are explicitly unsupported. | Added with the conservative 272K operational budget, 128K output metadata, image/function support, summaries, and strict effort gating in both authentication modes. |
| `gpt-6-sol` | Missing current visible API-supported row; minimum subscription catalog version 0.155.0. Official page supports `none` through `max`. | Added. API mode may expose `none`; subscription mode retains Vesper's policy of no `none`. |
| `gpt-6-luna` | Missing current visible API-supported row; minimum subscription catalog version 0.155.0. Official page supports `none` through `max`. | Added with the same auth-mode distinction. |
| Astra, GPT-5.6 Sol/Terra/Luna, GPT-5.5 | Present in Vesper and current pinned visible catalog. | Retained unchanged. |
| GPT-5.4, GPT-5.2, GPT-5.3 Codex, Codex Spark | Not current visible rows in the pinned Codex catalog, but remain evidence-backed capability rows and may still be returned by API account discovery. GPT-5.3 Codex has an announced April 1, 2027 shutdown, which has not occurred. | Retained to preserve valid saved selections and account-specific access. Discovery still excludes any row the active account does not return. |

OpenAI's pinned catalog also lists `ultra` for several models. Vesper does not expose
it: upstream describes it as host-owned automatic delegation, while Vesper's native
transport sends literal Responses efforts and has no Codex delegation runtime. The
public 1,050,000-token capacity is likewise distinct from Vesper's conservative
272,000-token operational input budget. Neither value was inferred from a model ID.

## Implementation

- Added `gpt-6.1-sol`, `gpt-6-sol`, and `gpt-6-luna` to the adapter-owned catalog.
- Added explicit per-model reasoning behavior:
  - GPT-6.1 Sol: `low` through `max`; reject `none`, `minimal`, and `ultra` in both modes.
  - GPT-6 Sol/Luna: API `none` plus `low` through `max`; subscription `low` through `max`.
- Preserved the 272K Vesper context budget and 128K output metadata.
- Advanced subscription discovery's independent protocol compatibility version from
  0.153.0 to 0.155.0, the minimum required by GPT-6 Sol/Luna in the pinned catalog.
- Kept discovery fail-closed: API and subscription results are intersected with the
  capability index; hidden, unknown, absent, malformed, failed, and stale rows cannot
  become choices or dispatch targets. The prerelease review added generation ordering
  so an older overlapping discovery cannot overwrite the latest-started snapshot.
- Unified subscription login/refresh persistence so refreshing subscription tokens
  preserves another separately stored valid API key without changing the selected
  subscription mode or introducing fallback billing.
- Added adapter tests for exact GPT-6.1 Sol metadata, image/tool-capable Responses
  serialization, 128K output boundary, and reasoning exclusions in both modes.
- Added current GPT-6 Sol/Luna auth-mode regressions and discovery coverage.
- Added ACP process evidence that GPT-6.1 Sol is advertised only from a returned
  account row, survives provider switch away/back, selects `max`, executes a real
  confined tool, returns the result to Responses, and completes a later turn.
- Updated TUI regressions so provider-owned model/effort metadata reaches the next
  native turn with GPT-6.1 Sol and its 272K operational budget.
- Updated the user guide, owning PRD, adapter DOX contract, foundation ownership,
  evidence index, and committed release-objective provenance in the same candidate.

No host-local model table, provider-name dispatch shortcut, Codex credential read,
Codex process, fallback billing route, live provider call, or user-state write was
introduced.

## Files

Production and hermetic tests:

- `crates/vesper-provider-openai/src/catalog.rs`
- `crates/vesper-provider-openai/src/credentials.rs`
- `crates/vesper-provider-openai/src/discovery.rs`
- `crates/vesper-provider-openai/src/factory.rs`
- `crates/vesper-provider-openai/src/tests.rs`
- `apps/agent-vesper-acp/tests/openai_native.rs`
- `apps/agent-vesper-tui/src/main.rs`

Documentation and contracts:

- `crates/vesper-provider-openai/AGENTS.md`
- `docs/openai-provider.md`
- `docs/openai-provider-prd.md`
- `docs/AGENTS.md`
- `docs/foundation/AGENTS.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/release-objective-provenance.json`
- `docs/foundation/2026-10-06-openai-catalog-audit-and-gpt-6-1-sol.md`
- `docs/foundation/2026-10-06-openai-catalog-prerelease-review-and-repair.md`

## Exact verification evidence

All counted commands ran from
`/home/Alex/Projects/agent-vesper/.worktrees/openai-gpt-6-1-sol` with synthetic
credentials and loopback fixtures only.

1. Focused adapter, ACP, and TUI proof:

   ```text
   cargo test -p vesper-provider-openai --all-features
   test result: ok. 62 passed; 0 failed; 0 ignored

   cargo test -p agent-vesper-acp --features integration-test-harness --test openai_native
   test result: ok. 6 passed; 0 failed; 0 ignored

   cargo test -p agent-vesper-tui openai_
   test result: ok. 3 passed; 0 failed; 0 ignored
   ```

   The original catalog candidate produced the 62-test receipt above. After the
   independent prerelease review repairs, the complete adapter suite produced:

   ```text
   cargo test -p vesper-provider-openai --all-features
   test result: ok. 64 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
   ```

   Named new receipts included:

   ```text
   tests::gpt_6_1_sol_metadata_and_responses_contract_are_exact ... ok
   tests::current_gpt_6_sol_and_luna_catalog_rows_preserve_api_none_only ... ok
   gpt_6_1_sol_native_subscription_tool_round_trip_uses_max ... ok
   tests::openai_palette_values_follow_model_and_authentication ... ok
   tests::native_openai_registry_and_model_controls_drive_the_shared_loop ... ok
   ```

2. Canonical workspace gate:

   ```text
   cargo xtask verify
   running: cargo fmt --all --check
   running: cargo clippy --workspace --all-targets --all-features -- -D warnings
   running: cargo test --workspace --all-features
   exit status: 0
   ```

   The OpenAI ACP process suite ran six cases inside this gate and passed. Existing
   container-runtime-only swarm cases remained explicitly ignored by their test
   contracts; they are unrelated to this provider catalog change.

   During final handoff, one initial `cargo xtask verify` rerun returned exit 1 after
   formatting and Clippy passed, but the bounded retained output ended before any
   failure marker. The complete workspace all-features command was rerun directly and
   exited 0 with no failure marker; the exact canonical command was then rerun in full
   and exited 0, including:

   ```text
   Acceptance regression gate: 100 exact cases passed in 104273 ms.
   Offline fixture model cost: zero; live-model effectiveness is not measured.
   ```

   The unexplained initial nonzero invocation is not counted as evidence and remains
   recorded here rather than silently replaced by the successful reproductions.

3. Rust 1.88 locked all-features gate:

   ```text
   cargo +1.88.0 test --workspace --all-features --locked
   Finished `test` profile [unoptimized + debuginfo] target(s) in 21m 39s
   exit status: 0

   CARGO_BUILD_JOBS=1 cargo +1.88.0 test --workspace --all-features --locked
   exit status: 0
   ```

   The bounded second command was the final handoff rerun. Its immediately preceding
   unbounded attempt exited 101 because GNU `ld` was killed by signal 9 while linking
   `agent-vesper-acp-test-driver`; no Rust source diagnostic or test assertion failed. Reducing
   only Cargo build concurrency produced the complete green locked suite. This host-
   resource failure is retained rather than represented as a passing source check.

4. Final documentation/source integrity:

   ```text
   cargo fmt --all -- --check
   git diff --check
   content checks: 7 Markdown files passed
   added/new local links: 2 passed
   added/new external links: 10 passed
   changed JSON files parsed: 1
   format and whitespace: passed
   ```

   The changed JSON file is the committed release-objective provenance binding;
   it parses successfully and points only at this report.

An earlier focused command was accidentally started from the original checkout,
identified by Cargo package paths under `/home/Alex/Projects/agent-vesper` rather
than this worktree. It is deliberately excluded from all evidence above. Every
counted receipt was rerun from the isolated worktree.

## Final audit

The final audit re-derived these invariants from source and primary evidence:

1. Production model metadata has one owner in `vesper-provider-openai::OpenAiCatalog`;
   TUI and ACP edits are regressions around the existing provider-port projection.
   No dependency manifest changed.
2. A model becomes selectable only when both the capability catalog and the active
   authenticated account snapshot contain it. Static support alone cannot establish
   account availability.
3. GPT-6.1 Sol's literal Vesper efforts are exactly low through max. None, minimal,
   and Codex-host ultra are rejected. GPT-6 Sol/Luna retain API-only none.
4. The 1,050,000 public context window, 872,000 pinned upstream maximum context,
   and Vesper's 272,000 operational input budget are separate claims. This change
   advertises only the Vesper operational budget to compaction.
5. Provider switching changes provider configuration, not OpenAI credential storage;
   switching back reuses the selected valid credential and revalidates saved model
   choices against the adapter/account surface.
6. Direct Responses serialization remains native and no Codex dependency or process
   was added.
7. A successful subscription refresh preserves a separately stored valid API key but
   leaves subscription selected; explicit logout still replaces the complete record.
8. Shared account availability is latest-started: an older concurrent discovery may
   return to its caller but cannot overwrite a newer snapshot or credential invalidation.

The independent prerelease review and both red-first repair receipts are recorded in
[the linked review report](2026-10-06-openai-catalog-prerelease-review-and-repair.md).
Its logout-token concern was disproved from replacement-write semantics and the existing
vault-canary test; no speculative logout change was made.

A test-only diff was applied to a detached checkout of pre-change base
`56f5ce6adfb26282f50350043beec5915a9c038d`. The corrected red command produced:

```text
verified catalog entry
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 61 filtered out
pre-change regression exit status: 101 (expected nonzero)
```

The same named regression on the candidate then produced:

```text
test tests::gpt_6_1_sol_metadata_and_responses_contract_are_exact ... ok
test result: ok. 1 passed; 0 failed; 0 ignored
```

The first red attempt used the short name with `--exact`, selected zero tests, and is
excluded. The detached checkout initially shared the candidate target directory; its
base-built test artifact contaminated Cargo's cache. The package-only cache was removed
with `cargo clean -p vesper-provider-openai` before the candidate green rerun. This
removed build artifacts only and changed no source.

A separate fail-fast primary-source audit re-fetched the exact pinned catalog, verified
its SHA-256 and all three added rows' visibility, API support, modalities, budgets,
minimum client versions and exact effort lists, then normalized the GPT-6.1 Sol public
page and verified its 1,050,000/128,000 limits, none/minimal rejection and Responses
tool route. Receipt:

```text
pinned-source invariant audit: 3 models passed; sha256 passed
GPT-6.1 Sol public-page invariant audit: limits/efforts/tool route passed
architecture audit: provider owns production metadata; host changes are regressions; dependency manifests unchanged
```

Narrative claims were checked against those receipts and the recorded code paths. The
readiness statement was kept explicitly local: hosted exact-commit and live-account
evidence remain unexecuted and are not represented as release proof.

## Constraints held

- No live OpenAI inference, discovery, authentication, or usage request.
- No writes to real credential stores, account state, or user configuration.
- No Codex CLI/app-server installation, bundling, launch, dependency, or credential read.
- Existing catalog rows, credential reuse, provider switching, saved selections, and
  restart-time adapter validation remain intact.
- API-key and ChatGPT subscription availability remain account-scoped and distinct.
- Public advertised context and Vesper's operational input budget remain distinct.
- No version bump, installer execution, push, tag, publication, or release.

## Deviations

- The audit expanded the implementation beyond the named `gpt-6.1-sol` row after
  finding two additional current visible, API-supported omissions (`gpt-6-sol` and
  `gpt-6-luna`) in the same pinned primary catalog. Shipping only the named row would
  have left the audited catalog knowingly stale.
- No live-account acceptance was attempted because foundation verification prohibits
  live provider calls and user-state writes.
- An initial whole-file link scan found pre-existing references in the large foundation
  index whose archived targets are absent from this branch. The candidate neither
  introduced nor repaired those unrelated baseline links. Every local and external
  link added by this change was then checked and passed as recorded above.

## Unresolved and release boundary

The repaired source passed the 64-test adapter suite, both-host focused suites, all
100 exact acceptance cases, canonical workspace verification and the Rust 1.88 MSRV
workspace suite. These local receipts precede version preparation and do not substitute
for the canonical, MSRV, five-target foundation and contained web-driver GitHub
workflows required on the exact pushed version commit before any immutable release tag.
Live account entitlement and service availability remain outside hermetic acceptance.
No release action has yet been performed.

## Readiness effect

The native OpenAI catalog now covers all current visible coding rows in the pinned
OpenAI catalog while preserving older evidence-backed account-specific choices. Both
hosts consume the same provider-owned metadata, and GPT-6.1 Sol has direct hermetic
catalog, discovery, reasoning, image/Responses, real-tool, provider-switch, and
next-turn evidence. The two review blockers have red-first focused repairs and the
repaired source passed acceptance, canonical and MSRV verification. The change is
locally ready for native RRC version preparation; exact-version hosted gates remain
mandatory before release.
