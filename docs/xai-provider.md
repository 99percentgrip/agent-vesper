# xAI / Grok provider

The native xAI adapter first shipped in v0.24.0. Major Audit 1 found an ordinary
Grok-session first-turn configuration defect and a browser-link UX defect in
that release. Both repairs passed live TUI/ACP acceptance and are included in
the v0.24.1 corrective release candidate. Audit 2 and Audit 3 remain open future
audit work.

## Authentication and billing

Open **Settings → Providers → xAI / Grok** and choose one mode:

- **Grok account / SuperGrok** uses the signed-in account's available
  Grok/Grok Build allowance. Browser sign-in is the normal path: Vesper opens
  the complete authorization URL, receives the loopback callback and stores its
  own refreshable credential. The modal can copy the exact complete link, and
  device-code sign-in remains the fallback.
- **xAI API key** uses separately billed xAI API credits. The TUI provides
  masked entry; headless ACP setup accepts `XAI_API_KEY` with
  `agent-vesper-acp --provider xai --setup`.

Vesper stores these credential classes separately. Authentication failure in
one mode never causes a request through the other. Vesper does not read
`~/.grok/auth.json` and does not install or launch Grok Build.

ACP supports `--provider xai --login`, `--device-login`, `--logout`, and
`--check-auth`. Both login routes store the same Vesper-owned Grok-session
credential used by TUI and ACP. Authentication messages use stderr so ACP
stdout remains pure JSON-RPC.

## Models and controls

After authentication, Vesper discovers models visible to that account and
intersects them with its verified xAI capability index. Unknown models remain
unselectable until their capabilities are verified. Model-specific reasoning
choices appear in Settings; the multi-agent beta labels its control as xAI-side
multi-agent scale.

Current verified image input is JPEG/JPG or PNG, at most 20 MiB per image. xAI
documents no image-count limit, so Vesper does not invent one. Grok 4.3 and 4.5
documentation currently conflicts about `xhigh`; Vesper fails closed rather
than presenting a distinct control whose semantics are not verified. Grok 4.5
therefore tops out at `high` because xAI's general reasoning guide says its
`xhigh` value is treated as `high`.

API-key mode can select Global or US regional processing. The regional route
restricts models to the documented regional set. HTTP/SSE is the correctness
transport; WebSocket is an explicit Global/API-key optimization. Native xAI
compaction is default-off and remains governed by Vesper's transactional
context policy.

Grok-account mode exposes the model and reasoning controls verified on the
subscription proxy. API region, WebSocket, native compaction and xAI-hosted
tool toggles are omitted because the current session protocol has not verified
those public-API capabilities. Vesper does not silently move such work to
separately billed API-key transport.

## Tools, citations, and privacy

Vesper file, shell, MCP, web, planning, skill, memory and worker tools continue
through the shared local tool loop and permission system. xAI Web Search,
X Search, Code Execution, Attachment Search, Collections Search, and Remote MCP
are separate, default-off provider-hosted controls. They run on xAI
infrastructure and may add egress or provider charges. Attachment Search uses
only the configured file IDs/public HTTPS URLs; Collections Search uses only
configured collection IDs. Remote MCP requires an explicit HTTPS URL and
label. xAI Code Execution never substitutes for Vesper `run_command`, and xAI
Remote MCP never inherits Vesper MCP enablement.

These xAI-hosted controls are currently available only in Global API-key mode.
They are not advertised for Grok-account/SuperGrok sessions after the verified
session proxy rejected a controlled hosted-tool request.

TUI Settings lists the provider-owned configuration rows and keeps them in the
normal draft until **Save changes**. ACP footer controls enable each hosted
tool; bounded structured values use the active provider commands
`/xai-file-ids`, `/xai-file-urls`, `/xai-collection-ids`,
`/xai-max-results`, `/xai-mcp-url`, `/xai-mcp-label`,
`/xai-mcp-description`, and `/xai-mcp-tools`. Lists are comma-separated.
Incomplete or unsafe configurations fail before provider dispatch.

Validated provider citations render in both hosts. Encrypted reasoning and
native compaction items remain opaque and are never displayed as chain of
thought.

Voice uses xAI only as the selected reasoning provider: local STT produces the
transcript, the normal AgentLoop dispatches it, and the configured TTS reads the
answer. The xAI speech and Imagine APIs are separate future integrations.

## Current acceptance boundary

Offline loopback coverage exists for the protocol contracts, discovery,
Responses and subscription transports, streaming, tools, structured output,
images, continuation, caching, hosted tools, compaction, WebSocket and both
hosts. The current Audit 1 candidate passes live browser and device login,
post-browser TUI/ACP text, `read_file` exactly once, `run_command` exactly once,
logout and browser reauthentication. The earlier `Missing or invalid
client_id` observation is an **INVALID TEST — manually copied URL was truncated
at terminal wrapping**. The v0.24.1 corrective release remains exact-commit
gated. Optional paid API-key acceptance was not run. See
[`foundation/vro18-audit1-completeness-and-capability-truth.md`](foundation/vro18-audit1-completeness-and-capability-truth.md).
