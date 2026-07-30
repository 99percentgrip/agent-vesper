# Reconnaissance Evidence Index

Status: COMPLETE

## Repository identity and state

| Repository | Confirmed evidence | Status |
|---|---|---|
| Target | `/home/alex/Projects/Agent Vesper`; contained only `AGENTS.md` at first inspection; not a Git repository | Inspected |
| Source | `/home/alex/Projects/Native GLM-5.2 Provider`; Git root matches path; `origin` is `https://github.com/99percentgrip/Native-GLM-ACP.git`; branch `agent/jit-tool-loading`; commit `bf4d4287e2e3320aa3f09015f678e6169d520045`; one pre-existing untracked `docs/codex-tui-roadmap-prompt.md` | Identity confirmed; preserve untracked user file |
| Package identity | `pyproject.toml` project `glm-acp`, Python 3.10+, Hatchling; `glm-acp = glm_acp.cli:main`; source ownership identifies `__main__.py -> cli.py:main() -> agent.py:run()` | Expected completed GLM ACP harness |

## Subsystem inspection ledger

| Subsystem | Source files / symbols | Tests | Confirmed behavior | Open questions | Documents updated | Status |
|---|---|---|---|---|---|---|
| Repository identity | `README.md`; `pyproject.toml`; root and subtree `AGENTS.md` | CI/tests not yet inspected | ACP-native Z.ai GLM harness with ACP, terminal, tools, persistence, learning, orchestration, and security surfaces | Release/CI/platform details and measured size pending | This index | Complete |
| Repository/build/release | `pyproject.toml`; `.github/workflows/{ci,quality,release}.yml`; `registry/agent.json`; installers | packaging/release tests identified | Python 3.10–3.13; five frozen targets; checksums/attestations/locked builds | Full local suite stalls as recorded below | inventory, architecture, migration, verdict | Complete |
| ACP/session/agent | `agent.py:Session`, `GlmAcpAgent`, lifecycle, loop and event helpers | `test_agent.py`, `test_quality.py`, `test_compaction.py` | Exact lifecycle capabilities, serialization, replay order, prompt locking, cancellation, tool/usage mapping, compaction and workers | Rust-independent fixtures do not exist yet | inventory, behavioral contract, parity, plan | Complete |
| GLM provider | `config.py` registries; `glm_client.py:GlmClient` request/stream methods | `test_glm_client.py`, `test_stream_integration.py` | Request shape, SSE order/separation, tool assembly, usage, retry, cancellation, continuation, quota host restriction | Cross-language fixture extraction is a blocker | inventory, behavioral contract, provider analysis, parity | Complete |
| Tools | `tools.py` schemas/dispatcher/implementations; `mcp.py` schemas | `test_tools.py`, `test_extensions.py`, roadmap tests | All tool names/classes, containment, bounds, timeout/process, patch transaction, MCP routing | Canonical JSON schema fixture still to be created | inventory, module map, security, parity, plan | Complete |
| Persistence | session/config/memory/telemetry/checkpoint/cron/plugin/worktree modules | respective tests | All discovered paths/formats/schema versions/readers/writers, core redaction/corruption/concurrency rules | `mcp.json` atomicity and session last-write-wins are known weaknesses | inventory, persistence, security, plan | Complete |
| Security | `security.py`, `os_sandbox.py`, `tools.py:Sandbox`, permission logic, hooks/plugins/checkpoints/mobile/MCP | security/roadmap/frontend tests | Permission precedence, path/env/process/OS/promptware/plugin/checkpoint/browser/mobile invariants | TOCTOU and real platform conformance remain migration risks | inventory, behavioral, security, risk | Complete |
| TUI/UX | `tui.py` screens/actions/client, terminal/voice/image/mobile dependencies | `test_tui.py`, `test_terminal_cli.py` | Screen families, shared state source, commands/settings/actions, terminal integrations/accessibility surfaces | Exact catalog/binding fixture is a blocker | inventory, behavioral, parity, architecture, plan | Complete |

## Commands executed

Commands are recorded exactly in logical command form; read-only commands run concurrently are listed separately.

1. `sed -n '1,240p' /home/alex/.agents/skills/docs-context7-first/SKILL.md`
2. In target: `pwd`; `git status --short`; `git remote -v`; `git branch --show-current`; `git log -1 --format='%H %s'`; `rg --files -g '!target' -g '!node_modules' | sed -n '1,240p'`
3. In source: `pwd`; `git rev-parse --show-toplevel`; `git status --short`; `git remote -v`; `git branch --show-current`; `git log -1 --format='%H %s'`; `rg --files -g '!build' -g '!dist' -g '!.venv' -g '!node_modules' | sed -n '1,280p'`
4. In target: `sed -n '1,260p' AGENTS.md`
5. In source: `sed -n '1,300p' AGENTS.md`
6. In source: `sed -n '1,300p' glm_acp/AGENTS.md`
7. In source: `sed -n '1,300p' tests/AGENTS.md`
8. In source: `wc -l AGENTS.md glm_acp/AGENTS.md tests/AGENTS.md pyproject.toml README.md`
9. In source: `sed -n '1,220p' glm_acp/AGENTS.md`
10. In source: `sed -n '1,220p' pyproject.toml`
11. In source: `sed -n '1,220p' README.md`
12. In source: `git ls-files | sed -n '1,260p'`
13. In source: `git ls-files 'glm_acp/*.py' 'tests/*.py' | xargs wc -l`
14. In source: `find . -maxdepth 3 -type f -not -path './.git/*' -not -path './.venv/*' -not -path './build/*' -not -path './dist/*' | sort | sed -n '1,320p'`
15. In source: `find .github registry scripts -maxdepth 3 -type f -print 2>/dev/null | sort`
16. In source: `du -sh . --exclude=.git --exclude=.venv --exclude=build --exclude=dist`
17. In source: `sed -n '1,240p' .github/AGENTS.md`
18. In source: `sed -n '1,280p' .github/workflows/ci.yml`
19. In source: `sed -n '1,280p' .github/workflows/quality.yml`
20. In source: `sed -n '1,360p' .github/workflows/release.yml`
21. In source: `sed -n '1,220p' registry/agent.json`
22. In source: `sed -n '1,260p' scripts/install.sh`
23. In source: `sed -n '1,280p' scripts/install.ps1`
24. In source: `rg -n '^(class |def |async def )' glm_acp/*.py`
25. In source: `rg -n '^(class |def |async def )' tests/*.py`
26. In source: `rg -n '^from glm_acp|^import glm_acp|from \\.|import_module\\(\"glm_acp' glm_acp tests`
27. In source: `rg -n '^(class |def |async def |    def |    async def )' glm_acp/agent.py`
28. In source: `rg -n '^(class |def |async def |    def |    async def )' glm_acp/tools.py`
29. In source: `rg -n '^(class |def |async def |    def |    async def )' glm_acp/glm_client.py`
30. In source: `rg -n '^(class |def |async def |    def |    async def )' glm_acp/tui.py`
31. In source: `nl -ba glm_acp/config.py | sed -n '1,180p'` and `nl -ba glm_acp/config.py | sed -n '390,780p'`
32. In source: `nl -ba glm_acp/agent.py | sed -n '250,760p'` and `nl -ba glm_acp/agent.py | sed -n '1820,2565p'`
33. In source: `nl -ba glm_acp/glm_client.py | sed -n '1,230p'` and `nl -ba glm_acp/glm_client.py | sed -n '500,804p'`
34. In source: `nl -ba glm_acp/tools.py | sed -n '1,180p'`, `nl -ba glm_acp/tools.py | sed -n '1120,1425p'`, and `nl -ba glm_acp/tools.py | sed -n '1480,2085p'`
35. In source: `rg -n '^TOOL_DEFINITIONS|^CRONJOB_TOOL_DEFINITION|^.*\"name\": \"' glm_acp/tools.py`
36. In source: `rg -n '^MCP_TOOL_DEFINITIONS|^DEFAULT_SERVERS|\"name\": \"' glm_acp/mcp.py`
37. In source: focused numbered reads of `tools.py:1160-1485`, `agent.py:1835-2560`, and `glm_client.py:110-230,510-805`
38. In source: persistence/path/schema search across session, config, memory, telemetry, checkpoints, cron, plugins, learning, hooks, MCP, and worktrees modules
39. In source: SQLite SQL search across `glm_acp/*.py`
40. In source: symbol search across persistence/security/orchestration modules
41. In source: numbered reads of `session_store.py:1-430`, `security.py:1-140`, `os_sandbox.py:1-240`, and `hooks.py:1-150`
42. In source: numbered focused reads of `session_store.py`, `checkpoints.py`, `plugins.py`, and `cron.py`
43. In source: numbered reads of ACP lifecycle, prompt, loop, permission and ACP-event helper ranges in `agent.py`.
44. In source: `nl -ba glm_acp/agent.py | sed -n '4768,4935p'` and focused loop ranges `2828-4005`.
45. In source: `nl -ba glm_acp/agent.py | sed -n '377,675p'`; `nl -ba glm_acp/session_store.py | sed -n '247,430p'`; persistence path search across `glm_acp/*.py`.
46. In source: isolated collection command `env HOME=/tmp/vesper-recon-home XDG_CONFIG_HOME=/tmp/vesper-recon-xdg PYTHONPYCACHEPREFIX=/tmp/vesper-recon-pycache PYTHONDONTWRITEBYTECODE=1 .venv/bin/python3 -m pytest -p no:cacheprovider --collect-only -q tests/`.
47. In source: isolated full suite with the same environment and `... pytest -p no:cacheprovider tests/ -q`; interrupted after it stopped progressing after 24 passes.
48. In target: `ps -eo pid,etime,stat,cmd | rg 'pytest|vesper-recon'` (process namespace did not expose the earlier execution process).
49. In source: `env HOME=/tmp/vesper-recon-home-2 XDG_CONFIG_HOME=/tmp/vesper-recon-xdg-2 PYTHONPYCACHEPREFIX=/tmp/vesper-recon-pycache-2 PYTHONDONTWRITEBYTECODE=1 timeout 180 .venv/bin/python3 -m pytest -p no:cacheprovider tests/test_agent.py -vv -x`; timed out at `TestConfigSwitch::test_auxiliary_model_switch_and_plan_fallback`.
50. In source: numbered read around that test plus exact-symbol search.
51. In source: numbered reads of config-switch implementation, test fixtures, agent save/client invalidation.
52. Final audit in target: file list, status/TODO search, required-document index search, non-recon file list, Rust/Cargo absence search, and line counts.
53. Final source invariance audit: `git status --short`; `git rev-parse HEAD`; `git branch --show-current`; `git remote -v`.
54. DOX/final closeout: reread target `AGENTS.md`, `docs/AGENTS.md`, and `docs/recon/AGENTS.md`; verify 13 mission Markdown reports, no incomplete status/placeholders, no Cargo/Rust production files; recheck source `git status --short` and `git rev-parse HEAD`.

## Documentation research

- Read the `docs-context7-first` skill before dependency recommendations.
- Context7 resolved and queried current Tokio 1.49, Reqwest 0.12.28 and Ratatui 0.30.2 documentation.
- Primary-source web research verified the official ACP Rust crate/repository (crate 2.0.0, stable wire protocol 1) and official MCP Rust SDK.

## Test result

- Collection: **879 tests collected**, passed collection in 5.08 seconds.
- Full run: **not completed**. First 24 tests passed, then no progress.
- Focused reproduction: first 24 `test_agent.py` cases passed; the next test, `TestConfigSwitch::test_auxiliary_model_switch_and_plan_fallback`, timed out at 180 seconds.
- No failure assertion was emitted. Root cause remains unresolved.
- Test state/cache/home were redirected under `/tmp`; source Git status and commit remained unchanged.

## Documents

| Document | Status |
|---|---|
| `evidence-index.md` | COMPLETE |
| `source-repository-inventory.md` | COMPLETE |
| `behavioral-contract.md` | COMPLETE |
| `python-to-rust-module-map.md` | COMPLETE |
| `provider-abstraction-analysis.md` | COMPLETE |
| `rust-architecture-proposal.md` | COMPLETE |
| `persistence-and-compatibility.md` | COMPLETE |
| `security-invariants.md` | COMPLETE |
| `parity-test-strategy.md` | COMPLETE |
| `performance-baseline-plan.md` | COMPLETE |
| `risk-register.md` | COMPLETE |
| `master-migration-plan.md` | COMPLETE |
| `executive-verdict.md` | COMPLETE |

## Closeout

- Source identity/state match the initial inspection: branch `agent/jit-tool-loading`, commit `bf4d4287e2e3320aa3f09015f678e6169d520045`, only the same pre-existing untracked `docs/codex-tui-roadmap-prompt.md`.
- Target changes are documentation and DOX only. No `Cargo.toml` or `.rs` file exists.
- No disposable analysis file was created.
- Final readiness: **READY WITH BLOCKERS**.
