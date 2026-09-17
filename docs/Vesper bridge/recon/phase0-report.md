# Vesper Bridge — Phase 0 Reconnaissance and Architecture Verdict (VB-PRD-001 rev 1.0)

## Objective

Execute Phase 0 of VB-PRD-001: inspect the current checkout and crate
contracts, refresh the external evidence base, compare candidate Resolve
and desktop-driver adapters, and deliver the evidence package — integration
map, adapter decision record, compatibility manifest, threat model and test
plan, and proposed ADRs — without producing core implementation.

Scope honored: no uncontrolled desktop operation, no software installation,
no purchases, no authentication changes, no release installation, no
mutation of real media/projects. No production stub was added. The tree
change from this phase is documentation + pinned read-only source evidence
under `docs/Vesper bridge/`.

## Methods and commands

- Baseline: `git status --porcelain=v1 -b`, `git log --oneline -5`,
  `git rev-parse HEAD`.
- Contract inspection: `grep`/`sed` over `crates/*/AGENTS.md`,
  `crates/vesper-agent/src/{registry,executor,permission,agent_loop}.rs`,
  `crates/vesper-mcp/src/mcp.rs`, `crates/vesper-harness/src/{lib,web_service,
  slash_commands}.rs`, `crates/vesper-domain/src/{tool,acceptance,content}.rs`,
  `xtask/src/main.rs` (architecture + naming-guard gates).
- Enrollment measurement: Rust-paragraph-count reproduction of
  `AcceptanceEnrollment::prepare` splitting (`\n\n`, empty-filtered).
- External refresh (curl, HTTPS only, read-only):
  `trycua/cua` main docs (driver README, platform support, Linux completion
  plan, MCP protocol), release API metadata for `cua-driver-rs-v0.28.1`,
  `samuelgursky/davinci-resolve-mcp` main docs (README, API coverage,
  21.1 typed API, scripting changelog, headless matrix/CLI, limitations,
  read-controls), `microsoft/playwright-mcp` README, MCP
  `2026-07-28` transports/versioning pages, XDG portal RemoteDesktop/ScreenCast
  pages, `blackmagicdesign.com` product/developer/support pages.
- Pinned copies with SHA-256 prefixes recorded in
  `recon/SOURCE-DIGESTS.txt`.
- Local environment probe: `uname`, `/etc/os-release`, `ps aux`
  compositor processes, `command -v` for resolve/ffmpeg/python.

## Files changed

- `docs/Vesper bridge/PRD-ENROLLMENT-NOTE.md` (enrollment-blocker record)
- `docs/Vesper bridge/recon/` (15 pinned upstream documents, digests,
  this report and its companions)
- `docs/foundation/evidence-index.md` (index entry)
- No source, config, or dependency changes.

## Exact evidence

### Baseline revalidation (§2 of the mission)

- HEAD `5a3ce20180e90cd001219e5c6734b5bb7c445208` on `main`,
  tracking `origin/main`, clean except the new `docs/Vesper bridge/`.
- Workspace version `0.23.0`, `rust-version = "1.88"` (`Cargo.toml:35,38`).
- The PRD's pinned baseline `75c1a550…` is an ancestor; `git diff
  75c1a550..HEAD --stat` shows 203 files / +308,818 −2,586 — the checkout is
  substantially beyond the research baseline, as the PRD anticipated. All
  integration map references below are to current HEAD, not the pinned commit.

### Enrollment gate — measured, BLOCKED

`acceptance_enroll` on the original file failed: the harness's enrollment
reader (`crates/vesper-harness/src/acceptance.rs`, `PRD requires 1–256 source
paragraphs`) counts PRD paragraphs **plus every `AGENTS.md` instruction file
along the workspace route**. Measured counts: PRD 234, `docs/AGENTS.md` 15,
root `AGENTS.md` 34 → **283 > 256**. Total bytes 174 KiB < 256 KiB cap.
`PRD-ENROLLMENT-NOTE.md` records this. No reduced or substitute PRD was
enrolled. Fixing the ceiling (or coalescing paragraphs losslessly) is an
explicit open item; until then native acceptance cannot freeze this PRD.

### Current integration map (file/symbol level, HEAD 5a3ce20)

| Concern | Owner | Reference |
|---|---|---|
| Tool registry + mode-filtered advertisement | `vesper-agent` | `crates/vesper-agent/src/registry.rs` — `ToolRegistry::parity_default` (line 65), `with_service` (133), `with_gateway` (169), `definitions_for(mode)` (211) |
| Hosted-tool seams (Bridge will use `with_service`) | `vesper-agent` | `registry.rs:36` imports `ToolService`; executor contract `crates/vesper-agent/src/executor.rs:193-206` (`ToolExecutor::definition/execute`) |
| Bounded image path (PRD BR-10/NF-06) | `vesper-agent` | `executor.rs:18` `MAX_TOOL_MEDIA_PARTS` = 8; `ToolResult.media` (90-93); `with_media` (120-131) — image parts only, ≤8 |
| Media → provider history | `vesper-agent` | `agent_loop.rs:1207,1269,1291,1557,1573,1731` |
| Image capability gate | `vesper-provider` | `model_capability.rs:103` `accepts_image` (fail-closed) |
| Permission gate (PRD BR-07/08) | `vesper-agent` | `permission.rs:165` `check_tool_permission(operating_mode, permission_mode, ToolExecutionClass)`; mode×permission×class decision matrix in tests; denial outranks bypass for firewall denials (`executor.rs:162-168` `FirewallDenial`) |
| Execution classes | `vesper-domain` | `tool.rs:109-124` `ReadOnly/Mutating/Shell/Process/Network/NestedWorkflow` |
| Cancellation | `vesper-agent` | `agent_loop.rs:547-749` `run_prompt_with_history_with_cancellation`, `CancellationSignal` port |
| MCP stdio + Streamable HTTP, scoped subprocess lifetime | `vesper-mcp` | `mcp.rs:342-347` `tools()` spawns per call; `McpProcess::spawn` (612-660); `MCP_PROTOCOL_VERSION = "2025-06-18"` (line 21) — legacy initialize flow only |
| MCP gateway (dynamic tool injection) | `vesper-harness` | `lib.rs:103` `MCP_GATEWAY_PREFIX = "mcp__"`, `McpGatewayExecutor` (160-237), registered via `build_default_registry` (2585) |
| Signed declarative-only plugins | `vesper-mcp` | `plugins.rs` (726 lines, Ed25519 loader; per `crates/AGENTS.md` unsigned path is debug-only) |
| Opt-in hosted service precedent (Bridge will mirror) | `vesper-harness` | `web_service.rs` `WebService`/`WebScope`, five opt-in web tools, `with_web_scope` (`lib.rs:2602`); swarm layer behind default-off `swarm` feature (`Cargo.toml:44-46`) |
| Session persistence | `vesper-sessions` | transactional `SessionWriter` (`writer.rs`), ACP-neutral replay plans |
| Sandbox | `vesper-sandbox` + `vesper-web-fetch` | ADR 0022 sole unsafe-code exception; network egress only inside `IsolationRequirement::Network` sandbox via helper binary |
| Architecture enforcement | `xtask` | `allowed_dependencies()` (`main.rs:1379+`), `architecture()` (916), naming guard (1177) — any `vesper-bridge` crate needs an explicit allowlist entry |
| Completion assurance | `vesper-harness` | `acceptance.rs`, ADR 0028; `cargo xtask acceptance` |

### External evidence refresh — what changed / was confirmed after cutoff

**Cua Driver** (`recon/cua-driver-readme.md`, sha `277e7123…`):
- MCP over stdio; `2026-07-28` modern revision **and** legacy
  `2025-06-18` initialize flow supported (shipped in Driver 0.28.0, PR
  #3609; `recon/cua-mcp-protocol.md` lines 1-49). HTTP endpoint stays
  legacy-only and rejects modern metadata — stdio required for modern.
- Permission modes: `standard` (promptless), `bounded` (manifest-admitted
  tools), `unrestricted` (`--dangerously-bypass-approvals`).
- Latest stable `cua-driver-rs-v0.28.1` published 2026-09-12; Linux
  x86_64 tarball ~30.5 MB with `checksums.txt`; skills archive ~83 KB.
  Nightlies to v0.28.2. Install scripts `install.sh`/`uninstall.sh` present.

**Cua platform matrix** (`recon/cua-platform.mdx`, sha `c0f25fd4…`):
- Linux X11: **Supported** with toolkit-specific limits (XTest + AT-SPI;
  synthetic-input rejection by some toolkits surfaces as structured refusal).
- Sway: **Supported with limits** (complete typed catalog).
- GNOME/Mutter: **Supported with limits** (needs Shell helper + one restart).
- **KDE/KWin: Experimental** — "Plasma 6 session startup, GTK AT-SPI
  discovery, generic discovery where exposed, and portal interface
  availability. The optional KWin helper exposes read-only window identity
  and state, **not activation or input**. Raw target-addressed input refuses
  until a target-bound KWin input path exists" (line 84).
- Hyprland/Omarchy: Experimental (isolated input v3, merged PR #3572,
  shipped 0.24.0, qualified applications only).
- XWayland: Supported with limits — capabilities depend on the app actually
  presenting an X11 window.
- Linux completion plan (`recon/cua-linux-plan.md`): KDE target is a
  "target-addressable KWin activation adapter using supported KWin
  scripting or D-Bus interfaces" (lines 242-246) — still a plan item, not
  shipped.

**MCP spec 2026-07-28** (`recon/mcp-transports.html` fetched live):
- No negotiation handshake; every request declares protocol version in
  `_meta` (`io.modelcontextprotocol/protocolVersion`), plus
  `MCP-Protocol-Version` header on HTTP. Servers answer
  `UnsupportedProtocolVersionError` (-32022) listing supported versions.
  Legacy (2025-11-25 and earlier) = initialize-handshake sessions; dual-era
  implementations support both.
- **Vesper's current client is legacy-only**: `vesper-mcp` pins
  `"2025-06-18"` and `initialize` → `notifications/initialized` →
  `tools/list`. Cua Driver's modern surface will still interoperate via its
  retained legacy path — verified compatible pair to pin in tests.

**Resolve / davinci-resolve-mcp** (`recon/resolve-mcp-readme.md`, sha
`a070ba04…`; v4.5.2):
- Uses the official scripting API; "full API coverage" = **361/361 methods
  covered, 338/361 live-tested**, tested against Studio 19.1.3.7 /
  20.3.2 / 21.0.2 and free 21.0.3.7 via the in-app bridge.
- **Free edition**: external scripting is Studio-gated. Through 21.0.x the
  ungated Workspace ▸ Scripts menu allowed an in-app bridge script
  (measured on free 21.0.3.7). **Resolve 21.1 moved Python scripting to
  Studio** — on free 21.1 the Scripts menu no longer lists `.py` files at
  all (Fedora 44 report, issue #203; Lua scripts still list). Free-edition
  bridge is therefore a 21.0.x path, unconfirmed on 21.1.
- Linux confirmed via user report on free 20.3.2.9 (Fedora 43), bridge at
  `~/.local/share/DaVinciResolve/Fusion/Scripts/Utility`.
- **Studio external scripting** requires Preferences → General → External
  scripting using → Local; on Linux the `resolve` script module
  (`/opt/resolve/libs/Fusion/` … `DaVinciResolveScript.py`) is reachable
  from system Python without the macOS PYTHON3HOME dance.
- Render headless vs GUI: "Headless is not a capability-reduced mode" —
  238 paired probes, zero capabilities that worked with UI and failed
  headless (`recon/resolve-headless-cli.md`). But several **silent-failure
  traps** are documented in `recon/resolve-limits.md` (149 KB): e.g.
  `ImportTimelineFromFile` returning `None` for duplicate timeline names
  without error; CreateProject discarding an unsaved current project;
  headless `SaveProjectAs` blocking forever on the never-saved
  "Untitled Project"; Fusion composition-lock writes that read back but
  never render. These are exactly the "driver success string ≠ postcondition"
  cases BR-12 targets.
- 21.1 additions (`recon/resolve-changelog.md`, dated 1 Sep 2026, copied
  from installed Studio 21.1.0.14): keyboard/project-settings/render presets,
  audio render APIs, transcription getters, multicam, timeline-item
  property getters, `AddTransition`, output blanking, audio normalization,
  clone media, DCTL validate/encrypt. Twelve 21.1 **read-only** controls
  live-measured on Studio 21.1.0.14 (`recon/resolve211-read.md`).
  `recon/resolve-typed.md` preserves the vendor
  `DaVinciResolveScript.pyi` snapshot (410 methods, 46 TypedDicts) with
  SHA-256 `00078fa1…` and provenance.
- Vendor pages: `blackmagicdesign.com/products/davinciresolve/studio`
  confirms "Python and LUA scripting … remote scripting API" for Studio
  (`recon/` — fetched live). The 21.1 assistant announcement (Creative COW
  syndication) is 403-gated from this machine; the PRD's boundary
  (vendor-native MCP bootstrap unverified) still stands. **Resolve is not
  installed on this machine** (`command -v resolve` empty, no `/opt/resolve`,
  no Flatpak entry), so the Resolve lane is BLOCKED at the local
  certification step regardless of route choice.

**Playwright MCP** (`recon/playwright-mcp-readme.md`, sha `2d5af3af…`):
- "not a security boundary"; `browser_run_code_unsafe` is explicitly
  RCE-equivalent (line 1055-1062) — must never enter the ordinary action
  allowlist (BR/§9.3). `--allowed-origins` explicitly not a security
  boundary either.

**XDG portals** (fetched live):
- RemoteDesktop v2: input via **EIS/libei recommended** (fd after
  `ConnectToEIS`) or D-Bus Notify* methods; both device-grant and session
  consent are user-facing.
- ScreenCast: "PipeWire node IDs can be reused after node destruction";
  `pipewire-serial` object.serial + `PW_KEY_TARGET_OBJECT` are the durable
  stream identity (added v5 of the interface; per-revision availability
  must be probed). Restore tokens are single-use.

### Adapter decision record (PRD §3.2)

| Route | Reused | Wrapped | Rejected | Unknown |
|---|---|---|---|---|
| Resolve Studio external scripting (official API via `DaVinciResolveScript` / Python) | Vesper tool/permission/media seams; ffprobe for verification | First-party Rust sidecar speaking the scripting transport; capability manifest distilled from the API surface | Direct project-DB SQL/XML mutation; undocumented endpoints; free-edition bridge as baseline (Studio-gated on 21.1) | Whether Studio is what Alex will install; exact installed build's behavior on Fedora 44 |
| Community davinci-resolve-mcp | Its **documented measurements** (silent-failure catalogue, headless matrix, 21.1 changelog) as evidence inputs; possibly its MCP server as a *leaf transport* behind Bridge tests | Capability subset selected from its 36-tool compound surface, mapped to typed Bridge operations | Adopting its tool surface as the Bridge API verbatim; its update-checker/auto-update paths | Its behavior on Studio 21.1 Linux (their evidence is macOS-primary + one Linux free-edition report) |
| Cua Driver | Its stdio MCP boundary with legacy `2025-06-18` handshake (matches Vesper's client today); its `bounded` permission mode concept informs our authority manifest | Wrapped as a **supervised leaf adapter** over stdio, never embedded/forked; executable pinned by digest from the v0.28.1 release `checksums.txt` | Generic desktop automation as the primary route; KWin input on this desktop (experimental, refuses target-addressed input) | Practical behavior of AT-SPI discovery + portal capture on Fedora 44 KDE without KWin activation |
| Vesper's existing browser route (`vesper-web`, `browser_ui`) | Preferred for browser applications unchanged | None needed for the second workflow candidate | A second browser driver; `browser_run_code_unsafe` anywhere in the allowlist | — |
| Vendor-native Resolve assistant/MCP | Watchlist only | — | Adopting without an accessible full manual or local install | Bootstrap, tool inventory, transport, licensing |

**Selected production candidate route:** first-party Rust sidecar over
Resolve Studio's **documented external scripting API**, with
davinci-resolve-mcp-style tool names as inspiration only, verified by
ffprobe + timeline readback. **Selected generic driver:** Cua Driver
v0.28.1 via stdio MCP in `bounded`-equivalent mode, for the
**Linux X11/XWayland lane only** on this machine (apps presenting real X11
windows), with KDE/Wayland-native surfaces explicitly uncertified.

**Why not KWin-native:** this host runs `kwin_wayland` on Fedora 44
(process list captured below). Cua's own matrix says KWin raw
target-addressed input refuses and activation is unimplemented; the Linux
completion plan treats a KWin activation adapter as future work. XWayland
remains viable for apps that present real X11 windows — that is the
certifiable lane here, and it must be labeled XWayland, never
"stock-Wayland".

### Compatibility manifest (initial, all statuses measured today)

| Item | Value | Status |
|---|---|---|
| OS/compositor | Fedora 44 (KDE), kernel 7.2.4, `kwin_wayland` + XWayland `:0` | Verified (`uname`, `ps`) |
| Rust toolchain | 1.95.0 stable; workspace MSRV 1.88 | Verified |
| Resolve | Not installed | **BLOCKED** — certification lane unavailable |
| ffmpeg/ffprobe | Present (`/usr/bin`) | Verified — verification tooling available |
| Python 3 | Present (system) | Verified — sidecar runtime feasible without new interpreter install |
| Cua Driver | v0.28.1 stable (2026-09-12), linux-x86_64 asset + checksums | Downloadable, not installed (not authorized) |
| Vesper MCP client | Legacy `2025-06-18` initialize flow only | Verified in source; Cua legacy path compatible |
| MCP target revision | `2026-07-28` (per-request `_meta`, no handshake) | Spec verified live; Vesper adoption = AD-07 decision, not yet implemented |

### Threat model and test plan (condensed; full mapping below)

Trust boundaries: model ↔ host policy; host ↔ driver sidecar (stdio,
sanitized env, pinned executable); driver ↔ OS portals/compositor (consent
surfaces); Bridge ↔ target application (ambient authority — see host-attached
caveat); Bridge ↔ provider (image egress is a separate permission).

Principal risks and owners:
1. **Prompt injection through observations** (subtitles, filenames, AX text,
   screenshot text) — owner: `vesper-bridge` observation contract + tests
   (AT-09). Delimited, never authority-bearing.
2. **Ambient desktop authority** — an existing app already has the user's
   full file/network rights; path validation in Bridge is not a sandbox
   (BR-22/§9.2). Owner: approval language + isolation decision points.
3. **Focus/geometry race on XWayland** — input may reach the wrong window if
   binding is stale. Owner: generation-bound binding + fresh-observation
   requirement (AT-03/AT-13). KWin-native surfaces: refuse.
4. **Timeout-after-commit / duplicate creates** — Resolve's documented
   silent failures make this the top Resolve-specific risk
   (`ImportTimelineFromFile` → `None` on duplicate name). Owner: intent
   journal + reconcile-before-retry + task-linked resource names (AT-18/19/20).
5. **Driver success ≠ postcondition** — owner: independent verification
   (ffprobe, timeline readback, rendered-sample checks) (AT-15/AT-27/28).
6. **Bypass via raw MCP gateway** (`mcp__` prefix tools bypassing Bridge
   policy) — owner: registry composition review; Bridge actions must not be
   reachable as un-gated MCP tools (AT-10).
7. **Held-input leak after stop** — owner: watchdog + emergency release path
   (AT-21/22, NF-02/03).
8. **Data egress** — screenshots to provider/remote driver are separate
   grants; clipboard off by default (AT-31).

BR/NF/AT mapping: full 30-BR × 14-NF × 44-AT matrix is drafted in
`recon/threat-model-and-test-plan.md` with implementation owners per row.
Phases: AT-01/02/06/07/10/21/22 are provable in Phase 1-2 with the fake
driver + host compositions; Resolve-dependent scenarios (AT-26-29) are
BLOCKED until a Studio install exists; AT-32 KWin portion is expected to
produce the designed refusal, which is itself the pass condition.

### ADRs proposed (to be accepted before Phase 1 code)

- AD-01 Bridge as native service + data-only profiles (no executable plugins)
- AD-02 Hybrid route ladder: native API → semantic AX/DOM → bounded visual
- AD-03 Host-attached vs isolated execution, fail-closed when isolation is
  demanded but unavailable
- AD-04 Owned application sessions + fenced exclusive mutation leases
- AD-05 Independent stop/admission closure + `unknown_outcome` settlement
- AD-06 Reuse of existing image/tool/media seams (8-image cap, capability gate)
- AD-07 MCP dual-era client: keep legacy pin for existing servers, add
  negotiated `2026-07-28` metadata path behind tests; no forced SDK swap
- AD-08 Non-destructive Resolve adapter over documented scripting API;
  no project-DB mutation; journal + reconcile; verification independent of
  API return values

Detailed phased change plan with gates: `recon/phased-plan.md`.

## Deviations

- Native enrollment could not complete (paragraph ceiling, above). The
  original PRD remains the frozen contract in the working tree; no
  substitute was enrolled. This is recorded as the primary blocker for any
  future "completed" status under enforced completion.
- Creative COW syndication of the Resolve 21.1 announcement returned 403
  from this machine; vendor pages were fetched directly instead. The
  assistant-integration bootstrap therefore remains unverified, consistent
  with the PRD's own boundary.
- Context7 MCP was unavailable in this session (`MCP server not found`); all
  upstream verification used direct pinned fetches instead, which satisfies
  the PRD's "verify against the upstream file/release" requirement.

## Unresolved items

1. **Enrollment ceiling repair** (harness `acceptance.rs`) — prerequisite
   for native acceptance of this PRD.
2. Resolve installation/edition decision on this machine — blocks the
   Phase 3 certification lane. Manual action: install Studio (licensed) or
   designate another certified machine.
3. Cua Driver 0.28.1 local install + XWayland lane certification — requires
   explicit authorization (sidecar executable, digest-pinned).
4. Whether an installed Portal (xdg-desktop-portal-kde) exposes ScreenCast
   v5 serial metadata on Fedora 44 — probe when capture work starts.
5. OS lane decision for the first certified generic lane (Linux X11/XWayland
   recommended; Windows/macOS later).

## Readiness effect

Phase 0 is complete per its gate: no invented APIs are load-bearing (every
external claim cites a pinned upstream file in `recon/`), executable
provenance for the driver is established (v0.28.1 release assets +
checksums), and the Resolve bootstrap question is reduced to "Studio
scripting API via sidecar" pending an actual install. Pure contract work
(`vesper-bridge` types/state machines + fake driver, Phase 1) may proceed;
platform lanes requiring the missing applications remain explicitly
blocked, not skipped.

## Verdict

**PASS (Phase 0 scope).** The architecture in the PRD is implementable on
the existing seams with one crate addition and no harness replacement.
The honest position: enrollment is mechanically blocked, Resolve is not
installed, and this desktop's compositor is the one Cua labels
experimental — so the first certified generic lane is X11/XWayland and the
first certified application lane is pending a Resolve install. Nothing in
this report certifies any application-control capability.

**Status legend for the acceptance matrix:** AT-01..05, 07..23, 30..44 —
planned, not executed (Phase 1+). AT-06, 21, 22 — partially observable via
existing host tests later. AT-26..29 — **BLOCKED** (no Resolve). AT-38 —
deferred (game phase). Every row's current state is **NOT TESTED** except
where a later phase supplies evidence.
