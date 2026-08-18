# Migration Status

Last updated: 2026-08-01

| Area | Status | Current evidence |
| --- | --- | --- |
| Reconnaissance, contracts, and frozen oracle | COMPLETE | `docs/recon/`, pinned Python oracle `bf4d4287e2e3320aa3f09015f678e6169d520045` |
| Provider-neutral domain/runtime/agent harness | IMPLEMENTED | Typed provider registry, session supervisor, multi-turn agent loop, permission gate, progress port, and hosted tool service |
| Z.ai GLM production adapter | IMPLEMENTED | Exact frozen six-model catalog, Coding/Standard/BigModel endpoints, auth, streaming, tools, reasoning, usage, image gating |
| ACP executable | IMPLEMENTED | Full harness is the production default; live ACP permission requests; persisted-session ports; setup/install lifecycle |
| Persistence, memory, checkpoints, MCP, plugins, workers | IMPLEMENTED | Durable bounded stores and typed hosted/slash-command bridges; no SQLite in checkpoints; unsigned plugins compile only in debug |
| Native TUI command surface | IMPLEMENTED | 83 registered routes including distinct `/export last`; no deferred fallback; missing routes fail closed |
| Native TUI interaction surface | IMPLEMENTED AND LOCALLY VERIFIED | Full palette/value pickers, mouse-operable footer/rows, reasoning/activity/TODO/report panels, working-tree views, Vim, keybinds, image/screenshot, sound, QR mobile approvals, and real optional local voice pipeline |
| Packaging/install/uninstall | IMPLEMENTED AND LOCALLY VERIFIED | Cross-platform scripts; checksum-verified local release installation for both binaries; release archives bundle the 81-skill seed library seeded non-destructively by both installers |
| Frozen-oracle implementation parity gate | PASSED LOCALLY | `docs/parity-audit-report.md`; AST command audit, `cargo xtask verify`, release structural audit, installer smoke, Cargo Deny, and RustSec audit are green |
| Multi-provider architecture | READY | Core/runtime/TUI controls are provider-neutral and provider-specific catalogs stay in adapters |
| Additional production providers | NOT CLAIMED | Z.ai is the only real production adapter currently registered; no provider/model is invented to inflate readiness |

The prior Stage 4/5-only text in this file was obsolete: the workspace now
contains the complete agent loop, hosted tools, persistence writers, memory,
checkpoints, MCP/plugins, worker composition, ACP harness, and native TUI.

“Multi-provider” currently describes the architecture and registration
contract, not a fabricated second provider. A new provider becomes production
support only after a real adapter supplies its own authentication, endpoints,
catalog, capabilities, request/stream mapping, fixtures, and CI evidence.
