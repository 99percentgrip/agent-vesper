# OpenAI catalog prerelease review and repair

Date: 2026-10-06

Branch: `feat/openai-gpt-6-1-sol`

Reviewed candidate: `6724ed1abaaf5370fa3a4f3d8b354a4320595451`

Base: `56f5ce6adfb26282f50350043beec5915a9c038d` (`origin/main`)

Verdict: **INDEPENDENT REVIEW FOUND TWO RELEASE-BLOCKING STATE DEFECTS. BOTH HAVE RED-FIRST REGRESSIONS AND LOCAL FIXES; ACCEPTANCE, CANONICAL AND MSRV GATES PASS. EXACT-VERSION/EXACT-SHA HOSTED RELEASE GATES REMAIN PENDING.**

## Objective

Run a separate Agent Vesper ACP review of the OpenAI coding-model catalog candidate before release, independently inspect the candidate’s metadata, discovery, credential, native-transport, both-host and evidence claims, resolve every confirmed defect, and preserve an exact record of disproved concerns and unexecuted gates.

## Independent review method

A separate installed Agent Vesper ACP process was started from the candidate worktree with a prompt that prohibited edits, commits, pushes, tags, releases and installation. Its source inspection identified three possible blocking defects:

1. subscription refresh appeared to replace the credential record without retaining a separately stored API key;
2. overlapping account-model discovery calls appeared able to publish out of start order;
3. logout appeared to retain subscription tokens behind a sign-out tombstone.

The reviewer could inspect source through read tools, but its exact Git-diff and primary-source shell requests were denied by the read-only permission path. A second clean ACP session was explicitly configured to the advertised `read` permission mode and a 24-tool ceiling. That session failed before producing a review response with:

```text
provider turn failed: QuotaOrRate
```

The parent review therefore rechecked each finding against the exact candidate/base diff and current source rather than treating the reviewer’s assertions as proof. Findings 1 and 2 were confirmed. Finding 3 was disproved: `Credentials::logout` calls `write(json!({"mode":"signed-out"}))`, and `write` replaces the stored JSON value rather than merging it, so the old token fields are removed. The existing isolated-storage test also verifies that the serialized vault contains none of the former API key or subscription token canaries after logout.

No reviewer process wrote repository files or performed release operations. The strict retry’s `QuotaOrRate` result is retained as a limitation, not represented as review success.

## Confirmed findings and repairs

### 1. High: subscription refresh deleted another valid authentication method

`Credentials::login` retained an existing `api_key`, but `Credentials::dispatch` serialized refreshed subscription tokens through `token_value` without merging that key. A normal successful refresh could therefore remove the stored API-key method. This contradicted the provider credential port’s selectable-method contract and the candidate’s credential-reuse claims.

Repair:

- replaced the two divergent serialization paths with `token_value_preserving_api_key`;
- both initial subscription login and refresh now preserve only a separately stored API key that still passes secret validation;
- selected mode remains `chatgpt`, so this does not introduce billing fallback;
- explicit provider logout still replaces the complete record with the sign-out tombstone.

### 2. High: older model discovery could overwrite a newer account snapshot

`OpenAiFactory::available_models` cleared and later published one shared `RwLock` snapshot without operation ordering. If discovery A started first, discovery B started later and completed first, then A completed last, A could replace B’s newer account list. Dispatch would then validate against stale account availability.

Repair:

- added one factory-scoped atomic availability generation shared by factory clones;
- every discovery start claims a new generation and clears availability only if it still owns that generation;
- completion publishes success or fail-closed emptiness only if no newer discovery or credential mutation has superseded it;
- credential storage, selection, login, clearing and logout invalidation advance the same generation before replacing the snapshot;
- callers still receive their own operation result, while shared dispatch authority remains latest-started.

## Red-first and green evidence

All commands ran from `/home/Alex/Projects/agent-vesper/.worktrees/openai-gpt-6-1-sol` with temporary storage or loopback fixtures only.

### Stale discovery regression before repair

```text
cargo test -p vesper-provider-openai --all-features discovery::transport_tests::older_discovery_completion_cannot_replace_the_newer_snapshot -- --exact --nocapture
running 1 test
test discovery::transport_tests::older_discovery_completion_cannot_replace_the_newer_snapshot ... FAILED
assertion failed: published.contains("gpt-6.1-sol")
test result: FAILED. 0 passed; 1 failed; 0 ignored; 62 filtered out
exit status: 101
```

The fixture held the first response, completed a later discovery with `gpt-6.1-sol`, then released the older `gpt-5.5` response. The unfixed code published the old response last.

### Credential-preservation regression before repair

```text
cargo test -p vesper-provider-openai --all-features credentials::tests::refreshed_subscription_record_preserves_the_saved_api_key -- --exact --nocapture
error[E0425]: cannot find function `token_value_preserving_api_key` in this scope
error: could not compile `vesper-provider-openai` (lib test) due to 1 previous error
exit status: 101
```

The red compile proves that the shared preserving persistence path required by the regression did not exist in the reviewed candidate.

### Focused green proof

```text
cargo test -p vesper-provider-openai --all-features credentials::tests::refreshed_subscription_record_preserves_the_saved_api_key -- --exact --nocapture
running 1 test
test credentials::tests::refreshed_subscription_record_preserves_the_saved_api_key ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 63 filtered out

cargo test -p vesper-provider-openai --all-features discovery::transport_tests::older_discovery_completion_cannot_replace_the_newer_snapshot -- --exact --nocapture
running 1 test
test discovery::transport_tests::older_discovery_completion_cannot_replace_the_newer_snapshot ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 63 filtered out
```

### Complete adapter green proof

```text
cargo test -p vesper-provider-openai --all-features
running 64 tests
test result: ok. 64 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
Doc-tests vesper_provider_openai
test result: ok. 0 passed; 0 failed
```

The suite includes the existing logout canary test, catalog metadata and effort checks, API/subscription discovery filtering, bounded authentication, native Responses serialization, SSE interruption safety, both-mode error classification and the two new regressions.

### Broad repaired-source green proof

```text
cargo xtask acceptance
Acceptance regression gate: 100 exact cases passed in 104505 ms.
Offline fixture model cost: zero; live-model effectiveness is not measured.
exit status: 0

cargo xtask verify
running: cargo fmt --all --check
running: cargo clippy --workspace --all-targets --all-features -- -D warnings
running: cargo test --workspace --all-features
exit status: 0

CARGO_BUILD_JOBS=1 cargo xtask msrv
running: rustup run 1.88.0 cargo test --workspace --all-features
Finished `test` profile [unoptimized + debuginfo] target(s) in 2m 53s
exit status: 0
```

The canonical and MSRV outputs include the six-case ACP native OpenAI process suite,
three TUI OpenAI wiring cases, 64 adapter cases and the repository’s explicitly ignored
container-runtime-only swarm cases. The ignored cases are not represented as passing
OpenAI or release evidence.

## Files

Production and regressions:

- `crates/vesper-provider-openai/src/credentials.rs`
- `crates/vesper-provider-openai/src/factory.rs`
- `crates/vesper-provider-openai/src/discovery.rs`

Contracts and evidence:

- `crates/vesper-provider-openai/AGENTS.md`
- `docs/foundation/2026-10-06-openai-catalog-prerelease-review-and-repair.md`
- `docs/foundation/2026-10-06-openai-catalog-audit-and-gpt-6-1-sol.md`
- `docs/foundation/evidence-index.md`
- `docs/foundation/AGENTS.md`
- `docs/AGENTS.md`
- `docs/openai-provider-prd.md`
- `docs/foundation/release-objective-provenance.json`

## Constraints held

- No live OpenAI authentication, discovery, usage or inference request was made by the repair verification.
- Tests used synthetic credentials, temporary private storage and loopback HTTP only.
- No Codex CLI/app-server was installed, bundled, launched or read.
- No production credential store, user configuration or installation was modified.
- No version mutation, push, tag, release, asset publication or Registry update occurred during this review-and-repair unit.
- Explicit logout still removes stored credentials; subscription failure never falls back to API billing.

## Deviations and limitations

- The independent reviewer’s first pass was source-capable but could not run its requested exact-diff or primary-source shell checks under the selected permission path. Its output was treated as leads only.
- The stricter read-only retry failed with `QuotaOrRate` before yielding a report. This does not invalidate the independently generated leads, but it means the parent’s exact-diff and test verification supplies the final confirmation.
- The credential defect was pre-existing in `origin/main`; the discovery race was also structurally present before the catalog-row change. Both still blocked this release objective because the candidate claimed credential reuse and stale-account exclusion.
- Complete workspace canonical, MSRV, both-host process, acceptance and hosted exact-SHA gates have not yet been rerun against the repaired source. The adapter result alone is not release proof.

## Unresolved items

1. Prepare the next unused version in the same implementation/release-candidate lineage.
2. Push that exact version commit and require successful canonical, MSRV, complete five-target foundation and contained web-driver workflows before tagging.
3. Publish exactly one immutable release, verify its assets, update the existing Registry PR in place, and retain post-publication receipts locally without installing the release.

## Readiness effect

The two confirmed state-integrity blockers now have deterministic regressions, complete adapter coverage, both-host focused proof, 100-case acceptance, canonical verification and Rust 1.88 MSRV proof. The logout concern is closed as disproved rather than changed speculatively. The source is locally ready for native RRC version preparation, but it is not authorized for tagging or publication until the required exact-version commit’s hosted gates pass.
