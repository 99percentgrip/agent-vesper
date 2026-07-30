# Source Test Stall Investigation

Status: COMPLETE

## Objective

Determine whether the previously observed stop at
`tests/test_agent.py::TestConfigSwitch::test_auxiliary_model_switch_and_plan_fallback`
is a reproducible source deadlock or a non-source execution problem, without
modifying the frozen source tree.

## Methods

- Reverified the source at commit
  `bf4d4287e2e3320aa3f09015f678e6169d520045`.
- Inspected `tests/test_agent.py:23-35,305-315`,
  `glm_acp/agent.py:449-525,698-704,1801-1811,2182-2312`, and
  `glm_acp/session_store.py:56-229`.
- Ran the focused test with isolated `HOME`, `XDG_CONFIG_HOME`,
  `XDG_CACHE_HOME`, `PYTHONPYCACHEPREFIX`, and `TMPDIR`, disabled the pytest
  cache provider, enabled bounded `faulthandler` dumps, and imposed a process
  timeout.
- Ran all of `tests/test_agent.py`, then the complete `tests/` suite under a
  fresh isolated state root and a 900-second outer timeout.
- Enumerated matching processes after completion and rechecked source Git
  identity/status.

## Exact implementation evidence

The test adds the fixture session to `GlmAcpAgent._sessions`, changes
`auxiliary_model`, then changes `api_endpoint` and expects an unsupported
vision auxiliary model to fall back to `main`
(`tests/test_agent.py:305-315`).

`set_config_option` serializes through `Session.prompt_lock`
(`glm_acp/agent.py:2182-2191`). The changed client key causes managed prompt
refresh, client invalidation, and a session save
(`glm_acp/agent.py:2301-2312`). Persistence is delegated to a worker thread
(`glm_acp/agent.py:698-704`); `SessionStore.save` performs bounded local JSON,
metadata, and fail-soft SQLite indexing (`glm_acp/session_store.py:193-231`).
No provider request occurs in this path.

## Commands and results

1. Focused diagnostic:

   `env HOME=/tmp/vesper-foundation-stall/home XDG_CONFIG_HOME=/tmp/vesper-foundation-stall/config XDG_CACHE_HOME=/tmp/vesper-foundation-stall/cache PYTHONPYCACHEPREFIX=/tmp/vesper-foundation-stall/pycache TMPDIR=/tmp/vesper-foundation-stall/tmp PYTHONDONTWRITEBYTECODE=1 PYTHONASYNCIODEBUG=1 timeout 45 .venv/bin/python3 -c 'import faulthandler,pytest; faulthandler.dump_traceback_later(8, repeat=True); raise SystemExit(pytest.main(["-p","no:cacheprovider","tests/test_agent.py::TestConfigSwitch::test_auxiliary_model_switch_and_plan_fallback","-vv","-s"]))'`

   Result: **1 passed in 1.43s**, normal exit; no delayed traceback fired.

2. Relevant file:

   `env HOME=/tmp/vesper-foundation-stall/home XDG_CONFIG_HOME=/tmp/vesper-foundation-stall/config XDG_CACHE_HOME=/tmp/vesper-foundation-stall/cache PYTHONPYCACHEPREFIX=/tmp/vesper-foundation-stall/pycache TMPDIR=/tmp/vesper-foundation-stall/tmp PYTHONDONTWRITEBYTECODE=1 timeout 300 .venv/bin/python3 -m pytest -p no:cacheprovider tests/test_agent.py -q`

   Result: **208 passed in 6.13s**, normal exit.

3. Historical-environment comparison, omitting the newly explicit cache/tmp
   roots, also passed: **1 passed in 1.40s**. Therefore omission of those two
   variables is not established as the prior cause.

4. Complete suite:

   `env HOME=/tmp/vesper-foundation-full/home XDG_CONFIG_HOME=/tmp/vesper-foundation-full/config XDG_CACHE_HOME=/tmp/vesper-foundation-full/cache PYTHONPYCACHEPREFIX=/tmp/vesper-foundation-full/pycache TMPDIR=/tmp/vesper-foundation-full/tmp PYTHONDONTWRITEBYTECODE=1 timeout 900 .venv/bin/python3 -m pytest -p no:cacheprovider tests/ -q`

   Result: **879 passed in 89.41s**, no failures or skips reported, normal
   process exit. A post-run process scan found no pytest, glm-acp, or fixture
   descendants other than the scan itself.

## Diagnosis

**Classification: reproduced baseline; historical stall not reproducible.**

There is no evidence of a deterministic source deadlock, provider/network
call, leaked fixture task, lock re-entry, or client shutdown failure in this
test. The complete suite now exits normally. The earlier reconnaissance run
was interrupted after its command stopped yielding progress, and its process
could not then be observed; consequently its external execution/harness cause
cannot be identified more narrowly after the fact.

Eliminated under the current commit and environment:

- deterministic session-lock deadlock;
- unmocked provider request in the affected path;
- deterministic `SessionStore`/FTS hang;
- test-order dependency within `test_agent.py`;
- complete-suite teardown leak.

The repeatable isolated invocation above is the source baseline command.

## Files inspected and created

- Inspected the source files cited above; no source file was written.
- Created only this target report.
- No diagnostic patch was necessary, so
  `docs/foundation/proposed-source-fix.patch` was intentionally not created.
- All persisted test state is under `/tmp/vesper-foundation-*`.

## Unresolved issue and readiness effect

The precise cause of the historical executor observation is unknowable without
its process/traceback. This is not a current migration blocker because the
focused, relevant-group, and full-suite reproductions all pass normally.

Local Linux proof only; this test investigation does not validate other
platforms.
