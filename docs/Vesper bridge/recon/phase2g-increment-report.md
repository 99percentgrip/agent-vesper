# Vesper Bridge — Increment 7: core-side bounded state (NF-05/07/08, AT-37)

## Objective

Close the last deterministic gaps in the Phase 1/2 contract layer: the
Bridge session core retained `operation records` and `outstanding jobs`
without any bound, with the comment "bounded by caller" — and the
**production-path journal** (`MemoryJournal` is the harness composition's
journal, not merely a test helper) was likewise unbounded. NF-05/07/08
and BR-04/13/18 (AT-37) make bounding a **core** obligation — a runaway
model loop or slow consumer must not be able to grow Bridge memory
without limit regardless of who drives it. "Bounded by caller" was
hidden coupling, not a contract.

## Methods and commands

- Red-first test `crates/vesper-bridge/tests/session_bounds.rs`:
  1. dispatching 2× the cap must leave records ≤ cap;
  2. enqueueing 2× the job cap must leave jobs ≤ cap;
  3. bounding must evict (oldest first), never refuse — session stays
     `Ready`/`Open` and the newest record survives.
- Verified red on the pre-repair code by neutralizing the two mechanisms
  in place: **3/3 failed**; restored, **53/53 pass**.
- Core changes in `session.rs`:
  - `MAX_OPERATION_RECORDS = 512`, `MAX_OUTSTANDING_JOBS = 256`;
  - eviction after insert (oldest BTreeMap key first), dispatch never
    refused by the cap (BR-04 lifecycle);
  - job list keeps the newest window.
- **Second real bug found by this work:** request ids were derived from
  `records.len() + 1`. Under eviction the map length saturates at the
  cap, so ids would collide with journal entries and trip BR-16
  duplicate suppression after ~512 operations — a latent failure the
  unbounded code could never expose. Ids are now a monotonic
  `request_sequence` counter (u64 overflow fails closed via
  `ResourceLimit`).
- Third closure (same turn): `MemoryJournal` — the journal the harness
  composition actually installs — now bounds its own retention
  (`MAX_RECORDS = 512`, newest window kept, oldest evicted first; an
  evicted id reads as `None`/Fresh, the honest answer once no intent
  survives, and writes at the cap still land). Red-first via
  `tests/journal_bounds.rs` (2 tests).

## Files changed

- `crates/vesper-bridge/src/session.rs` — caps, eviction, job window,
  monotonic `request_sequence`, `latest_observation_revision()` getter.
- `crates/vesper-bridge/src/journal.rs` — `MemoryJournal::MAX_RECORDS`
  bound with newest-window eviction.
- `crates/vesper-bridge/tests/session_bounds.rs` — new (3 tests).
- `crates/vesper-bridge/tests/journal_bounds.rs` — new (2 tests).

## Exact evidence

| Item | Test/observation | Result |
|---|---|---|
| Records bounded by core | `operation_records_are_bounded_by_the_core` | PASS (red on pre-repair code) |
| Jobs bounded by core | `outstanding_jobs_are_bounded_by_the_core` | PASS (red on pre-repair code) |
| Eviction, not refusal | `bounds_are_eviction_not_refusal` | PASS (red on pre-repair code) |
| Journal bounded (production path) | `memory_journal_bounds_its_retention` | PASS (red: cap did not exist) |
| Evicted ids stay honest, writes land at cap | `evicted_ids_read_as_fresh_not_as_errors` | PASS |
| No existing contract regressed | `cargo test -p vesper-bridge` | **55 passed, 0 failed** (50 prior + 5 new) |
| Harness unaffected | `cargo test -p vesper-harness --features bridge --lib` | 137 passed, 0 failed |
| clippy both toolchains | bridge+harness combo `-D warnings` | 0 errors (1.88.0 and stable) |
| MSRV | `cargo +1.88.0 check -p vesper-bridge --tests` | clean |
| fmt / architecture / naming-guard / acceptance | repo gates | clean / 28 pkgs / 33 frozen / 23/23 |
| Pinned sources intact | `SOURCE-DIGESTS.txt` verification script | 16/16 digests match, 0 missing |
| Enrollment probe (ignored lane) | `real_vesper_bridge_prd_opens_under_the_raised_ceiling` | PASS under `--ignored` |

## Honest status

- The enrollment `--ignored` sweep also re-ran
  `real_dependency_browser_readiness`, which requires
  `VESPER_DEPENDENCY_TEST_ENGINE`/`_IMAGE` env vars and is an explicit
  contained-runtime lane — **NOT TESTED here**, by design, not silently
  skipped: it is the same class as the blocked application lanes (needs
  a supplied pinned engine).
- AT-37's host-level budget telemetry (queue depths, supersede-not-drop
  observation queues) remains **NOT TESTED** — that is harness/adapter
  territory and lands with the adapter phases. This increment closes the
  core-state half.
- Live-application lanes remain BLOCKED (Resolve install; Cua
  authorization). No capability is claimed beyond the deterministic
  contract evidence above.

## Deviations

- The eviction policy (oldest-first) is the PRD-compatible choice: NF
  budgets require bounding; BR-04 requires the lifecycle keep working.
  Newest-window retention is the audit-relevant end (recent intents for
  reconcile/duplicate checks). Journal-side retention is unchanged —
  `JournalPort` implementations own their persistence bounds.

## Unresolved items

1. DaVinci Resolve Studio installation (Alex) — Phase 3.
2. Cua 0.28.1 install authorization (Alex) — Phase 4.
3. Native enrollment (hosting-process restart; no `acceptance_enroll`
   on this session's tool surface).
4. AT-37 host-level observation queues/telemetry (adapter phase).

## Readiness effect

The Bridge core now bounds its own memory on every axis — session
records, outstanding jobs, the production-path journal, and pixels
(already capped at 16,777,216/image-package) — and the latent
request-id collision bug that only eviction could expose is fixed
before any adapter ever runs a long session.
