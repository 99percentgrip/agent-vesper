# VRO-18.1 Settings authentication — execution report

## Objective

Make provider authentication a working Settings route for every registered
provider, reusing provider credential ports, without a new provider or a
second auth implementation behind `/auth`.

## Baseline

- Requested inspected commit: `52d36202226c1932be9d8304a32e88c4a40cd3ce`
  (`origin/main`, Cargo version 0.24.1).
- The workspace `main` checkout was 38 commits behind that commit and had
  unrelated documentation edits. Those edits were stashed as
  `unrelated docs before vro18.1 settings auth`. One untracked foundation
  note that already existed upstream was moved to
  `/home/Alex/Projects/agent-vesper-wip-hold/`.
- Implementation branch: `vro18.1/settings-auth` at the inspected baseline
  plus this working tree. No commit, push, tag, or install was performed.

## What changed

- `ProviderCredentialPort` gained inventory, method selection, method-scoped
  store/clear, and an explicit removal scope. Defaults do not pretend a no-op
  save succeeded.
- xAI and OpenAI credential documents keep the unselected method's secret when
  the other method is stored or selected. Logout still writes the signed-out
  tombstone and does not fall through to an environment key.
- Z.ai inventory distinguishes stored and environment keys. Logout removes only
  the stored key.
- LM Studio `store_credential` persists. Test builds use an in-memory backend
  so `cargo test` does not touch the OS keyring. Production builds use
  `SecureCredentialStore`. The optional method is what allows "no authentication
  required"; an empty descriptor does not.
- Settings → Providers renders **Manage authentication**. Enter/Space on a
  provider still only updates the draft. **M** manages the highlighted row.
  `/auth` and startup sign-in call `run_authentication_panel`.
- A Grok-session override clears stale API-only hosted tools even if the
  startup surface still advertises them.

## Commands and results

Build directory: `/home/Alex/Projects/agent-vesper-target-vro18.1` (disk-backed).
Measured size after the focused builds: 9.6G. It was left in place so the
candidate can be rebuilt incrementally. It was not installed.

- `cargo test --offline -p agent-vesper-tui --lib auth_settings::` → **5 passed**.
  Covers Settings reachability for an unfamiliar provider id, xAI-style
  subscription → API key → subscription through the Settings event loop while
  OpenAI's store stays unchanged, Z.ai without browser/device rows, optional
  LM Studio wording, cancelled key entry, and a complete sign-in URL that
  rejects an embedded newline.
- `cargo test --offline -p agent-vesper-tui --lib provider_hub::` → **2 passed**.
- `cargo test --offline -p agent-vesper-tui --lib optional_api_key_persistence` → **passed**.
  A no-op save would fail: the stored key is present on the next session config
  and logout removes it.
- `cargo test --offline -p vesper-provider-xai --lib explicit_api_key_mode` → **passed**.
  Dispatch bearer follows API key, then the preserved Grok session, then the
  API key again.
- `cargo test --offline -p vesper-provider-openai --lib credentials::` → **3 passed**.
- `cargo test --offline -p agent-vesper-tui --bin agent-vesper-tui grok_session_turn_ignores` → **passed**.
- `cargo clippy --offline -p agent-vesper-tui --all-targets -- -D warnings` → **pass**.
- `cargo clippy --offline -p agent-vesper-acp --all-targets -- -D warnings` → **pass**.
- `cargo clippy --offline -p vesper-provider -p vesper-auth -p vesper-provider-xai -p vesper-provider-openai -p vesper-provider-glm --all-targets -- -D warnings` → **pass**.
- `cargo fmt` on those packages → clean.

## Mutation checks

Each edit was restored immediately. The suite was green afterward.

1. Disconnect **M** (`if false &&`). `single_method_panel_has_no_browser_and_optional_auth_is_truthful` **FAILED**.
2. Route Manage authentication to `providers.first()` instead of the highlighted provider. `two_way_switch_and_inactive_provider_isolation` **FAILED** (`assertion left == right`).
3. Drop the committed `select_authentication_method` result before the panel finishes. The same two-way test **FAILED**.

## Deviations

- Local `main` was not the inspected baseline. Work was done on a branch from
  `origin/main` so the xAI provider existed. The user's unrelated doc stash was
  not reapplied.
- Automated xAI/OpenAI two-way proof uses an injected in-memory credential port
  with the advertised method shapes, plus the real xAI credential store's
  dispatch test. It does not call live xAI or OpenAI and does not touch the
  developer keyring.
- LM Studio's production OS-keyring path is compiled only outside `cfg(test)`.
  Tests prove the port and session route with the injected backend.
- `cargo xtask verify` was not invoked as one command. Its constituent gates
  that were actually run are listed below. Five-target CI and `cargo audit`
  were not completed in this work unit.
- Version remains 0.24.1. No commit, push, tag, or install.

## Later gates (same working tree)

- `cargo fmt --all --check` → pass (empty output, exit 0).
- `CARGO_TARGET_DIR=/home/Alex/Projects/agent-vesper-target-vro18.1 cargo clippy --offline --workspace --all-targets --all-features -- -D warnings` → pass in 13.99s (`CLIPPY_EXIT:0` in `/tmp/vro181-clippy.txt`).
- `cargo xtask acceptance` → **23/23** exact cases, 70457 ms (`/tmp/vro181-acceptance.txt`).
- `cargo xtask` architecture → `architecture boundaries validated for 31 packages`.
- naming-guard → `clean (36 hits, all frozen in baseline)`.
- `cargo deny` → `advisories ok, bans ok, licenses ok, sources ok`.
- `cargo xtask msrv` (`rustup run 1.88.0 cargo test --workspace --all-features`) → log `/tmp/vro181-msrv.txt` ends on successful doc-tests; `test result: FAILED` count is 0; `test result: ok` count is 194.
- Real TUI process: `python3 apps/agent-vesper-tui/tests/settings_auth_pty.py /home/Alex/Projects/agent-vesper-candidates/vro18.1/agent-vesper-tui` → **PASS**. Isolated HOME and signed-out xAI/OpenAI/LM Studio vaults. The screen showed both xAI methods and both OpenAI methods, masked the typed key, and Cancel returned to the landing screen without creating a provider preference file.
- `python3 apps/agent-vesper-tui/tests/xai_plain_turn_pty.py` against the **debug** binary `/home/Alex/Projects/agent-vesper-target-vro18.1/debug/agent-vesper-tui` → **PASS** (loopback, one Grok-session dispatch). The same script against the release candidate **fails closed** with "Selected xAI model is not in the current verified account model list". That is the production `validate_availability` path: `integration-test-harness` is off in release, so the loopback fixture is not treated as a verified account catalog. It is not a release-candidate transport receipt.

## Candidate

Source: HEAD `52d36202226c1932be9d8304a32e88c4a40cd3ce` plus the uncommitted branch diff. `auth_settings.rs` and `settings_auth_pty.py` are untracked, so `git diff` alone does not hash them. Release binaries were built after those files and were not installed.

- `/home/Alex/Projects/agent-vesper-candidates/vro18.1/agent-vesper-tui`
  SHA-256 `58822c67a4b01af72a963aa25ea794b5a8cc4f3c7c3775878593e096cba738ae` (26,216,112 bytes)
  `--version` prints `agent-vesper-tui 0.24.1`
- `/home/Alex/Projects/agent-vesper-candidates/vro18.1/agent-vesper-acp`
  SHA-256 `cc350df69664df4bd916d61a45fe7b3c76454e58a9e5abef656cf8d8554ef672` (24,811,472 bytes)

Launch:

```text
/home/Alex/Projects/agent-vesper-candidates/vro18.1/agent-vesper-tui
```

This candidate uses the same OS keyring service as the installed app. Sign-out or key replacement changes the real stored credential. The process test above did not do that.

## Unresolved

- Alex has not personally clicked the route. The process test above is the automated reachability receipt, not his acceptance. USER SETTINGS ACCEPTANCE: **PENDING**.
- Paid API live acceptance: **NOT RUN**.
- `cargo audit` and five-target release CI: **NOT RUN**.
- Audit 2 and Audit 3 were not prerequisites and remain open.
- Release: **not published**. Installed app: **unchanged**. Version bump to 0.24.2 waits for acceptance and release authorization.

## Readiness

Settings authentication is implemented, and the release candidate's Settings route was exercised by a real TUI process. That is not a release and not a substitute for Alex's own acceptance.

## Navigation correction (user acceptance)

Alex's recording: Down from highlighted xAI crossed Z.ai and the single shared
"Manage authentication" row followed Z.ai. M on xAI already worked.

Each provider row now owns the next row, `Manage authentication · <id>`.
Down from that provider opens that provider. Active provider, draft choice,
focus, and the authentication target stay separate. M uses the same target as
the focused provider or its action row.

Red proof before the fix: `arrow_down_opens_that_provider_not_the_next_one`
failed with focus on `[x] zai` and `Manage authentication · zai`.

After the fix:

- `cargo test -p agent-vesper-tui --lib auth_settings::` → 10 passed
- `cargo test -p agent-vesper-tui --lib provider_hub::` → 3 passed
- `python3 apps/agent-vesper-tui/tests/settings_auth_pty.py` against the new release candidate → both PASS lines (arrow path and masked click path)

Updated candidate, still 0.24.1, not installed:

- `/home/Alex/Projects/agent-vesper-candidates/vro18.1/agent-vesper-tui`
  SHA-256 `cdf7a94c82c0aa2e2188d8ead4d2729a58dba227f739dbcfa45a8c31ce9483d6`
- ACP candidate was not rebuilt; previous hash remains
  `cc350df69664df4bd916d61a45fe7b3c76454e58a9e5abef656cf8d8554ef672`

Release is still waiting for acceptance of this navigation retest.


## Screenshot check (user report: manage still opens Z.ai)

The reported screen is OpenAI checked, xAI active, cursor on `Manage authentication · xai`.

Replayed on the candidate binary `cdf7a94c82c0aa2e2188d8ead4d2729a58dba227f739dbcfa45a8c31ce9483d6`:

- Enter on `Manage authentication · openai` opens a panel whose title is OpenAI. `Z.ai GLM` is absent.
- Click on `Manage authentication · xai` opens `Settings › Providers › xAI / Grok › Authentication`. `Z.ai GLM` is absent.
- Click-coordinate test `click_on_each_manage_row_resolves_to_that_provider` maps every visible manage row to that provider at 120×40, 100×30, 80×24, and 60×18.

The process still running from Sep 26 18:07 is `/home/Alex/.local/share/agent-vesper/agent-vesper-tui` (SHA-256 `7838e3a8367878501714f3b84455d94969defd70f241c97e8a1a61ff6b237d1a`). That installed file does not contain the per-provider manage rows. It was not replaced.


## Row-to-title capture on candidate cdf7a94c

Candidate `/home/Alex/Projects/agent-vesper-candidates/vro18.1/agent-vesper-tui`, SHA-256 `cdf7a94c82c0aa2e2188d8ead4d2729a58dba227f739dbcfa45a8c31ce9483d6`. Installed binary not replaced. Live processes at capture time were not this file: PID 22916 (deleted exe `c4f29171…`, no manage-row string) and PID 2617434 (`7838e3a8…`, installed, no manage-row string).

Keyboard, highlight recorded immediately before Enter:

| Before | Title after |
| --- | --- |
| `Manage authentication · xai` (one Down from the xAI provider row) | `Settings › Providers › xAI / Grok › Authentication` |
| same row after Back, Enter again | `Settings › Providers › xAI / Grok › Authentication` |
| `Manage authentication · openai` | `Settings › Providers › OpenAI › Authentication` |
| `Manage authentication · zai` | `Settings › Providers › Z.ai GLM › Authentication` |

Clicks on those three action labels produced the same three titles. No xAI action opened Z.ai.

`Z.ai GLM` appears only when the activated row is `Manage authentication · zai`. From the highlighted xAI provider row that is Down three times, then Enter. Down once then Enter names xAI / Grok.


## Keyboard repeat (recording 49.826–51.155)

Frames, not a routing miss:

- 49.826 `Manage authentication · xai`
- 50.032 zai provider row
- 50.514 `Manage authentication · zai`
- 51.155 `Settings › Providers › Z.ai GLM › Authentication`

Desktop repeat is delay 500ms, then every 30ms (`gsettings` keyboard delay/interval). The provider menu treated `KeyEventKind::Repeat` as another Down. A held Down therefore walked off the xAI action onto Z.ai's action before Enter.

Red test `held_down_repeat_stays_on_the_xai_action` failed on that sequence (opened Z.ai), then passed after the menu ignored repeat and release for navigation keys. Character and Backspace repeat still edit an API-key field. Mouse move does not change the row. While the settings reader is open, Konsole is asked to report press/repeat/release (`PushKeyboardEnhancementFlags`); the flag is popped when that reader drops.

Candidate after this fix: `/home/Alex/Projects/agent-vesper-candidates/vro18.1/agent-vesper-tui` SHA-256 `09d541b9c2f43cf9665e9068e29b5ae0a62ba792a5057709f5d3ac6535c04dcb`. Observed on that binary: one Down from the xAI provider row, two repeat sequences, release, and a mouse-move, still highlighted `Manage authentication · xai`; Enter opened xAI / Grok. Trace lines were `press Down` then `press Enter` only.
