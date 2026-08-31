# Migration Status

Last updated: 2026-08-31

| Area | Status | Current evidence |
| --- | --- | --- |
| Reconnaissance, contracts, and frozen oracle | COMPLETE | `docs/recon/`, pinned Python oracle `bf4d4287e2e3320aa3f09015f678e6169d520045` |
| Provider-neutral domain/runtime/agent harness | IMPLEMENTED | Typed provider registry, session supervisor, multi-turn agent loop, permission gate, progress port, hosted tool service, result-aware loop detection, truthful incomplete-terminal classification, autonomous plan continuation, transactional interruption outcomes, and a bounded catalog-backed full-payload capability gate with typed same-provider suggestions shared by ACP/TUI |
| Z.ai GLM production adapter | IMPLEMENTED | Evidence-backed eight-model catalog shared by ACP/TUI, including GLM-5.3-Flash native image input (URL/Base64), 1M context, 128K output, model-specific reasoning and plan gates, Coding/Standard/BigModel endpoints, auth, streaming tools/reasoning/usage, interruption classification, bounded safe continuation, and no-replay ambiguous-tool handling |
| ACP executable | IMPLEMENTED | Full harness is the production default; mixed and image-only ACP content is preserved; capability failures name the active model and catalog-verified alternatives through the existing selector; live permissions, persistence, setup/install, native plans, actionable failures, slash parity, and streaming tool correlation remain implemented |
| Persistence, memory, checkpoints, MCP, plugins, workers | IMPLEMENTED | Durable bounded stores and typed hosted/slash-command bridges; no SQLite in checkpoints; unsigned plugins compile only in debug |
| Native TUI command surface | IMPLEMENTED | 83 registered routes including distinct `/export last`; no deferred fallback; missing routes fail closed |
| Native TUI interaction surface | IMPLEMENTED AND LOCALLY VERIFIED | Full palette/value pickers, mouse-operable footer/rows, reasoning/activity/TODO/report panels, working-tree views, Vim, keybinds, clipboard images, and explicit catalog-backed capable-model consent that preserves/resumes the multimodal turn; sound, QR approvals, and optional local voice remain implemented |
| Packaging/install/uninstall | IMPLEMENTED AND LOCALLY VERIFIED | Cross-platform scripts; checksum-verified local release installation for both binaries; release archives bundle the 93-skill seed library seeded non-destructively by both installers |
| Frozen-oracle implementation parity gate | PASSED LOCALLY | `docs/parity-audit-report.md`; AST command audit, `cargo xtask verify`, release structural audit, installer smoke, Cargo Deny, and RustSec audit are green |
| Multi-provider architecture | READY | Core/runtime/TUI controls are provider-neutral and provider-specific catalogs stay in adapters |
| Additional production providers | IMPLEMENTED | LM Studio is the second real registered adapter, with native model discovery and capability-gated controls; no other provider/model is claimed |

The prior Stage 4/5-only text in this file was obsolete: the workspace now
contains the complete agent loop, hosted tools, persistence writers, memory,
checkpoints, MCP/plugins, worker composition, ACP harness, and native TUI.

“Multi-provider” describes both the provider-neutral architecture and the two
currently registered real adapters: Z.ai and LM Studio. Any further provider
becomes production support only after a real adapter supplies its own
authentication, endpoints, catalog, capabilities, request/stream mapping,
fixtures, and CI evidence.
