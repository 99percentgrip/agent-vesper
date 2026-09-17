# Vesper Bridge — Product Requirements Document

## 1. Executive decision

**Build Bridge as a provider-neutral application-control subsystem inside Agent Vesper, with pluggable application adapters and supervised external drivers. Do not make Bridge synonymous with MCP, a screenshot loop, or an executable package in the existing plugin loader.**

Bridge will let a person describe work in ordinary language, select an application or approved workspace, and have Vesper inspect, plan, act, verify and report the result. The flagship workflow is non-destructive video editing in DaVinci Resolve. A second, unrelated application workflow must demonstrate that the architecture is genuinely reusable. Game control is a separate experimental workstream, not evidence that arbitrary desktop tasks or competitive real-time play are solved.

The product promise is: **one consistent way to delegate work across explicitly supported applications, with truthful capability discovery, controlled execution and verifiable outcomes.** “Any software” is the architectural ambition, not a launch compatibility claim. Installing a driver does not establish application support; support requires a tested combination of application version, edition, operating system, control route and task.

| Control item | Definition |
|---|---|
| Specification identifier | VB-PRD-001, revision 1.0 |
| Decision status | Proposed architecture; ready for implementation reconnaissance and design review |
| Evidence cutoff | 13 September 2026 |
| Repository baseline | `99percentgrip/agent-vesper`, commit `75c1a5508055f0a373f9ca335b64c414dc97a393` |
| Verified foundation | Workspace version 0.22.3; Rust edition 2024; MSRV 1.88; Apache-2.0 [^S01] |
| Primary surfaces | Existing TUI and ACP host compositions |
| Initial deployment | Local workstation; optional, disabled by default |
| Implementation status | This document specifies proposed behavior. No Bridge integration or application-control test has been executed as part of this specification. |

The first implementation should retain Vesper's existing agent loop, provider adapters, permission infrastructure, image-capability gating, hosted services and completion-assurance mechanisms. It should add the smallest new contract and execution layer necessary to control applications. A replacement harness, a parallel provider loop, a new dashboard and a plugin marketplace are not prerequisites.

## 2. Evidence and architectural implications

### 2.1 Existing Vesper integration points

The current repository is substantially beyond its historical Stage 5 architecture. Its documentation index explicitly distinguishes current usage and implementation status from older specifications. The pinned workspace and current crate contracts, rather than the historical diagram alone, form the integration baseline. [^S30]

The inspected `ToolResult` implementation already carries text, dynamically injected tool definitions, bounded image content and optional text-file change previews. `with_media` accepts only image parts and enforces a maximum of eight. Bridge should use this path rather than invent an independent vision conversation or turn screenshots into prose. A Resolve edit is not a text-file diff and must not misuse `FileChangePreview`. [^S04]

The documented MCP subsystem already supports bounded stdio and Streamable HTTP. Its release plugin loader requires trusted Ed25519 signatures, and its plugins are intentionally declarative: prompt context, policy templates and workflows, not executable code. The documented `tools()` inspection path has a scoped subprocess lifetime and is synchronous. Long-lived Bridge connections therefore require an explicit lifetime design; they must not be assumed to exist simply because MCP discovery works. [^S03]

The agent layer owns tool execution and permission gating, while shared hosted services live in `vesper-harness`. Both are appropriate integration seams. Image requirements are checked before provider dispatch, and worker restrictions remove executable registrations rather than merely hiding tool schemas. Bridge must preserve these mechanisms. [^S05][^S06]

### 2.2 Open-source and standards findings

| Candidate | Evidence-backed contribution | Recommended disposition |
|---|---|---|
| Cua Driver | A Rust driver with CLI/MCP and SDK integration paths; a practical reusable desktop-control boundary. [^S07] | Evaluate as the first generic driver behind Vesper-owned contracts. Start out of process; do not fork or embed its SDK until compatibility is demonstrated. |
| Microsoft UFO | Separates desktop application control from higher-level orchestration and combines API and GUI actions. [^S10] | Adopt architectural patterns, not a competing agent runtime inside Vesper. |
| Microsoft Playwright MCP | Browser automation through structured accessibility information; explicitly not a security boundary. [^S11] | Reuse Vesper's existing browser integration where suitable. Keep browser profiles and network confinement explicit. |
| Resolve community MCP | Existing scripting integration and version-specific compatibility reports; also includes advanced DB/XML manipulation. [^S19] | Evaluate only the documented live scripting subset. Exclude direct project-database and undocumented file mutation from the baseline. |
| MCP | Standard discovery and tool invocation, with a current dated revision of 2026-07-28. [^S12] | Use as an interoperability transport, not as Bridge's product model or authority system. |
| Official Rust MCP SDK | Current client source distinguishes legacy initialization from discovery-oriented lifecycle modes. [^S16] | Reference for compatibility work. Adopting a published SDK version requires a separate MSRV, dependency and behavior evaluation. |
| Cradle | Research prior art for screenshots, keyboard/mouse control and application/game-specific skills. [^S24] | Inform experimental skill execution; do not import production-readiness claims. |
| ViZDoom | A visual research environment with controlled synchronous and asynchronous interaction. [^S25] | Use for the first game-control proof, with appropriate engine and asset licenses. |
| OSWorld 2.0 | A long-horizon evaluation framework with a recommended, versioned August 2026 benchmark release. [^S26] | Add a pinned evaluation subset after deterministic Bridge contract tests exist. |
| OpenTimelineIO and ffprobe | Timeline interchange representation and machine-readable media inspection respectively. [^S27][^S28] | Optional interchange and verification components, not substitutes for Resolve's editing engine. |

**Build-versus-adopt decision:** build Vesper's authority, session, verification, scheduling and user-experience layers. Adopt replaceable drivers and supported application interfaces. Avoid creating a new OS automation stack, browser engine, video editor or foundational model as part of Bridge v1.

### 2.3 Important current limitations

Cua's detailed platform matrix is more restrictive than a generic “cross-platform” claim. It distinguishes X11, Sway, GNOME/Mutter and KDE/KWin. KWin is experimental in the inspected matrix, and arbitrary raw input to an occluded native Wayland surface is not a general capability. A nested compositor's success does not establish the same behavior on a normal desktop. [^S08][^S09]

Windows input injection is constrained by process integrity. A successful input-submission API call is also not proof that the application reached the intended state. These are reasons to prefer application APIs and semantic actions and to verify results independently. [^S22]

The current MCP transport specification uses request metadata and explicitly describes interoperability with earlier initialization-based revisions. An implementation based only on older examples can therefore be incompatible with newer peers, while forcing the newest behavior on legacy servers can also break working integrations. [^S13]

### 2.4 Resolve 21.1 evidence boundary

Blackmagic's 8 September 2026 announcement, available in syndicated form, confirms assistant integration for tasks including project analysis, media organization, settings and batch rendering. Vendor product documentation also describes Python/Lua scripting and workflow integration. [^S18][^S17]

The exact Resolve 21.1 connection bootstrap, exposed tool inventory, protocol revision and edition-specific restrictions were not established from an accessible full vendor manual. They are **blocking validation items for choosing the production Resolve adapter**, not details to invent. The design therefore prefers a vendor-native connector where the installed version documents and exposes one, and otherwise evaluates sanctioned scripting. It does not prescribe an unverified endpoint, port, tool count or API method.

Community reports about older free editions must not be treated as proof of compatibility with 21.1 or later. A supported route must pass local version/edition discovery and documented-interface tests. Neither license circumvention nor automatic downgrading is an acceptable compatibility strategy.

## 3. Product scope and user outcomes

### 3.1 Primary journeys

**Video-editing delegation.** A person supplies local footage and an editorial brief: “Create a 60-second highlight edit, add a title, balance the audio, apply the approved look and export a draft.” Vesper identifies the exact Resolve instance, checks available operations, proposes an edit plan and works in a new project or verified duplicate. It returns an actual preview and export-validation report, distinguishing technical checks from subjective editorial approval.

**General application task.** A person asks Vesper to operate an unrelated application, for example enter supplied data into a dedicated browser application and export a report. The same discovery, permissions, action records, cancellation and verification mechanisms must work without adding application-specific branches to the core engine.

**Permitted game experiment.** A person asks Vesper to operate a local test game. Bridge establishes an observation/input session, runs bounded actions and reports objective state or episode results. The initial environment is a controlled reference game, not a public competitive match.

**Human takeover.** At any point, the person can stop Bridge, inspect its last verified state and continue manually. Resume requires a fresh observation and renewed validation of relevant authority and resource identity.

### 3.2 Scope tiers

| Tier | Included |
|---|---|
| Initial engineering proof | Pure contracts, fake driver, current-host integration, app identity, permission checks, bounded observations, independent stop and deterministic tests. |
| First useful release | One validated Resolve route, non-destructive editing subset, local artifact verification, generic driver integration on one certified OS lane, TUI/ACP parity. |
| Generality release | A second unrelated application, capability-aware fallback, documented platform matrix, persistent connection recovery, reviewed workflow reuse. |
| Later extensions | Additional application adapters, carefully scoped multi-application workflows, optional remote workers and game-control experiments. |

Non-goals are unrestricted autonomous use of the entire desktop, universal effect automation, guaranteed professional artistic judgment, real-time cloud-model reflexes, public multiplayer automation, anti-cheat evasion, hidden background control, arbitrary plugin code execution, and replacing Vesper's existing coding functionality.

## 4. Functional requirements

“Must” denotes a release requirement for the owning phase. “Should” denotes a desirable improvement that may be deferred only with an explicit reason. The numbered tests in Section 15 establish how these requirements are accepted.

| ID | Required behavior |
|---|---|
| BR-01 Discovery | Enumerate configured adapters and explicitly discover local application candidates. Return identity, version evidence, control routes and availability. Passive discovery must not install software, start the target app or alter its preferences. |
| BR-02 Exact attachment | Bind a session to a particular process generation, application identity and window/resource. Ambiguous matches must be resolved explicitly; title text alone is insufficient. |
| BR-03 Capability negotiation | Expose versioned, machine-readable operations, observation formats, delivery modes, safety requirements, verification options and support evidence. Unsupported and unknown capabilities must remain distinct. |
| BR-04 Connection lifecycle | Own connect, health, detach, shutdown and recovery. Application sessions must survive ordinary model turns without pretending that a transport connection is the application session. |
| BR-05 Skills separation | Select relevant skills automatically from the request and available capabilities. Skill text may guide planning but may not grant permissions, install adapters or expand tool access. |
| BR-06 Planning | Produce a plan linked to required capabilities and observable completion criteria. Missing critical capabilities must block that plan or produce a clearly reduced alternative. |
| BR-07 Mode enforcement | Preserve existing plan/read-only restrictions. Launching apps, changing focus, clicking, typing, editing projects and starting exports must not be smuggled through read-only tools. |
| BR-08 Authorization | Evaluate authority immediately before every executable step, including steps inside approved batches and fallback routes. Denial outranks model suggestions, skills and convenience settings. |
| BR-09 Observation | Provide bounded semantic state and, when permitted, images. Include target identity, timestamps, geometry, coordinate mapping and a freshness revision. |
| BR-10 Model compatibility | Use the current provider-neutral image path and capability gate. A text-only model may use semantic tools; it must not receive silently discarded images or be represented as seeing them. |
| BR-11 Action execution | Execute validated typed operations, not free-form shell text or arbitrary application script evaluation. Return submitted, applied and verified outcomes separately. |
| BR-12 Verification | Evaluate postconditions independently of the executor's success string. Retain the evidence necessary to explain completion, partial completion and uncertainty. |
| BR-13 Long-running jobs | Return a job identity for renders and other lengthy work; support bounded progress observation, cancellation requests and truthful settlement status. |
| BR-14 Concurrency | Serialize conflicting writes and global input ownership. Parallel sub-agents may plan or inspect only within their granted scopes; they must not compete for one cursor. |
| BR-15 Stale-state protection | Revalidate target, relevant state revision, authority generation and lease before dispatch. Changed focus, window geometry, document identity or app generation can invalidate a planned action. |
| BR-16 Duplicate protection | Suppress duplicate accepted requests locally. After uncertain dispatch, reconcile actual state before retrying any non-idempotent operation. |
| BR-17 Stop and takeover | Offer a human-accessible stop path independent of model inference. Stop new admission, revoke leases and release driver-owned held inputs; report any job that may still be running. |
| BR-18 Recovery | Recover inspectable state after transport/app failure without automatically replaying prior mutations. Quarantine sessions with uncertain cleanup or unresolved queued writes. |
| BR-19 Non-destructive media | Protect original media, use a new/duplicated project or timeline, record changes and require explicit approval before destructive replacements. |
| BR-20 Export delivery | Deliver a real output file and validation summary. Submission to a render queue alone does not satisfy export completion. |
| BR-21 Host parity | Expose equivalent authority, execution and result semantics through TUI and ACP. Presentation differences must not alter permissions or success classification. |
| BR-22 Privacy | Make screenshot, audio, metadata and file transmission destinations explicit. Do not send local content to a provider or remote driver merely because that service is configured. |
| BR-23 Adapter integrity | Load signed declarative profiles through the existing trust path. Executable sidecars require separate installation/provenance controls and may not be embedded as an undocumented plugin capability. |
| BR-24 Auditability | Record bounded, secret-safe request identities, decisions, app/adapter versions, job status, verification and failure causes. Do not store hidden model reasoning or continuous desktop recordings by default. |
| BR-25 Diagnostics | Provide actionable diagnostics for missing apps, unsupported versions, permissions, protocol mismatch, inaccessible media, missing effects and unsupported delivery modes. |
| BR-26 Workflow learning | Save a reusable workflow only after verification and explicit learning policy. Bind it to compatible capability/schema versions; never replay captured pixels or secrets as durable authority. |
| BR-27 Generality | Demonstrate at least two unrelated application workflows through the same core contracts before describing Bridge as a general application-control release. |
| BR-28 Extension safety | An adapter update that changes executable identity, tool schema, authority scope or compatibility guarantees must invalidate affected cached approvals and certification. |
| BR-29 Accessibility and usability | All essential controls must be available without relying only on color, animation or mouse access. Terminal output must remain bounded and readable. |
| BR-30 Feature isolation | With Bridge disabled, existing coding, MCP, provider and host behavior must remain compatible; no driver launch, screen capture or new provider request is allowed at startup. |

## 5. System architecture

### 5.1 Logical structure

The following is a proposed architecture, not a depiction of an already implemented subsystem.

```text
User request / TUI / ACP
          |
Existing Vesper agent loop and skill selection
          |
Tool registry + permission gate + completion assurance
          |
Bridge service composed in vesper-harness
  | identity / session / leases / observations / verification |
          |
Provider-neutral Bridge ports and contracts
          |
  +------------------+------------------+------------------+
  | App-native route | Semantic UI route| Visual/input route|
  | vendor MCP/API   | DOM / AX / UIA   | capture + driver  |
  | approved SDK    | AT-SPI           | bounded actions   |
  +------------------+------------------+------------------+
          |
Exact application instance / isolated workspace / job
          |
Postcondition verification -> evidence -> agent continuation

Independent human stop -> admission gate and driver watchdog
```

The model decides what task to attempt. The skill supplies domain procedure. The Bridge planner selects an available control route. Policy determines whether the operation is permitted. The adapter translates it. The driver or application performs it. A verifier evaluates the result. These responsibilities must not collapse into a single “run whatever the model emitted” tool.

### 5.2 Minimal native integration

| Component | Proposed responsibility |
|---|---|
| New `crates/vesper-bridge` | Provider-neutral identities, manifests, pure state transitions, operation/result types, support classifications, lease decisions and adapter/verifier ports. Initial workspace dependencies: domain and security only, subject to architecture review. |
| Existing `vesper-harness` | Compose Bridge services, supervise owned driver processes, connect existing transports, resolve host authority, schedule background work and route progress to both hosts. Prefer modules before adding multiple adapter crates. |
| Existing `vesper-agent` | Register Bridge executors and deferred schemas; use existing permission, provider-capability, history and cancellation paths. Avoid a second agent loop. |
| Existing `vesper-mcp` | Reuse transport/configuration and signed data-only plugin infrastructure. Add only reviewed session-lifetime or protocol-compatibility work that the reconnaissance proves necessary. |
| Existing `vesper-sessions` | Persist minimal Bridge lineage through an approved versioned extension or storage port. Do not place OS handles, live approvals or executable replay instructions in session history. |
| Existing `vesper-checkpoints` | Reuse workspace snapshot concepts where applicable. Application-native backups remain a separate adapter operation; filesystem snapshots do not automatically restore application databases or external renders. |
| Existing `vesper-observability` | Secret-safe telemetry and aggregate reliability metrics, using current opt-in behavior. |
| Existing TUI and ACP applications | Thin composition, target selection, consent, progress, stop and review. No provider or adapter internals in presentation code. |

This ownership proposal follows the current workspace boundaries, including the narrowly scoped unsafe-code exception and the existing placement of persistence and sandbox services. Changes require explicit dependency-graph and architecture-test updates, not broad exemptions. [^S02]

External application SDKs may require Python or Node in supervised adapter processes. That does not justify rewriting the Rust harness in those languages. Conversely, “pure Rust harness” must not be misrepresented as “every target application's extension mechanism is Rust.” Dependency installation remains explicit and optional.

### 5.3 Route selection

Prefer a supported native application operation, then a semantic accessibility/DOM operation, and finally permitted visual input. Selection must also consider verification quality, effective permissions, target identity, latency and whether human focus would be disturbed. A native route is not automatically safe merely because it is native.

Fallback is a new execution decision. For example, failure to trim a clip through an API does not authorize clicking arbitrary coordinates or evaluating a script. A fallback must meet the same plan, permission, resource and verification constraints. An unavailable safe route returns a typed refusal and a useful manual instruction.

## 6. Contracts and capability discovery

### 6.1 Identity model

Keep `chat_session_id`, `run_id`, tool-call identity, `bridge_session_id`, application instance, document/project identity, resource lease and asynchronous job identity separate. A reconnect may create a new transport connection while preserving an inspectable Bridge session, but it must not silently preserve authority across a changed process generation.

An application binding should contain an OS-specific stable application identity where available, executable identity, PID plus process-start evidence, window identity, user/desktop session and any adapter-specific project identifier. IDs and window handles can be reused; no single raw integer is durable identity.

For capture streams, use the strongest identity available in the negotiated portal version. Current ScreenCast documentation specifically warns that PipeWire node IDs can be reused and describes stronger stream targeting metadata. Do not assume that every installed portal already implements that revision. [^S21]

### 6.2 Capability manifest

A capability record must include its namespaced operation name, schema revision/hash, application/version/edition constraints, route type, support level, observation and verification methods, mutability class, required authority, idempotency category, cancellation semantics and known limitations. The catalog should also carry the provenance and date of its last accepted behavioral test.

Use separate fields for **availability** (`available`, `permission_required`, `dependency_missing`, `unavailable`) and **implementation status** (`native`, `emulated`, `unsupported`, `unknown`). An operation can be natively supported but currently unauthorized. An emulated operation can be available without being equally reliable.

The requested operation set is the intersection of the adapter's capabilities, application state, host permissions, session policy, worker restrictions and model input capabilities. A signed manifest is an authenticated declaration, not proof that its advertised operation works.

### 6.3 Proposed Bridge tool surface

The initial model-facing surface should stay small: `bridge_discover`, `bridge_connect`, `bridge_capabilities`, `bridge_observe`, `bridge_execute`, `bridge_job_status`, `bridge_verify` and `bridge_disconnect`. Human stop is also exposed outside the model tool channel. These names are **proposed Vesper tools**, not existing Cua, MCP or Resolve API names.

`bridge_execute` must select from validated typed operations. It must not contain arbitrary `code`, `shell`, `eval`, unrestricted URLs or opaque command strings. Deferred application-specific schemas may be injected through Vesper's existing mechanism, but execution must still resolve through the same Bridge policy gate.

A tool description or MCP annotation cannot establish its true risk class. Compound tools must be classified per selected action. Discovery, schema validation and structured results should reuse MCP semantics where applicable, while Bridge adds application identity, authority and evidence requirements. [^S14]

### 6.4 Illustrative operation envelope

This example defines the shape of a proposed internal contract. Names and values are illustrative; it is not an executable Resolve API request.

```json
{
  "bridge_schema": "1.0",
  "request_id": "host-generated-request-id",
  "bridge_session_id": "host-generated-session-id",
  "application_instance_id": "generation-bound-application-id",
  "operation": "media.timeline.create",
  "capability_generation": 3,
  "authority_generation": 2,
  "lease_token": "opaque-host-issued-reference",
  "precondition": {
    "observation_id": "observation-42",
    "resource_revision": "project-revision-7"
  },
  "arguments": {
    "project_ref": "approved-working-copy",
    "name": "Draft 01"
  },
  "execution_policy": {
    "timeout_ms": 15000,
    "retry_class": "reconcile_before_retry"
  }
}
```

The model must not choose authoritative identity, lease or approval fields. Host code creates and validates them after resolving the proposed operation. Result envelopes contain outcome state, affected resource references, effect summary, job reference if any, verification status, evidence references and a bounded diagnostic. They do not expose credentials or internal raw paths unnecessarily.

### 6.5 Protocol and transport rules

Bridge schema versioning is independent of MCP revisioning. Preserve working legacy peer behavior while introducing a tested route for the 2026-07-28 specification. Pin and test both sides of every supported compatibility pair; do not assume that a current SDK main branch is a released, MSRV-compatible package.

Prefer inherited stdio for a locally launched sidecar. Use permission-restricted local IPC when an independent worker is required. HTTP is explicit and opt-in; loopback binding alone is not authentication. Remote transport requires authenticated endpoint identity, protected credentials and explicit content-egress permission. OAuth transport access is not permission to mutate an application. [^S15]

### 6.6 Error contract

Define provider-neutral errors for `target_ambiguous`, `target_changed`, `capability_unavailable`, `permission_required`, `permission_denied`, `stale_observation`, `lease_conflict`, `schema_mismatch`, `transport_unavailable`, `resource_limit`, `verification_failed`, `unknown_outcome` and `cleanup_unconfirmed`. Each error includes retry eligibility, the last known effect state and a bounded recovery instruction. A generic driver exception must not erase whether a mutation could already have happened.

## 7. Observation and provider integration

### 7.1 Observation package

An observation should carry semantic application state first: active project, selected resource, visible controls, relevant job states and accessible element references. Images supplement that state where necessary. Capture the smallest sufficient surface rather than the entire desktop by default.

Every image must identify its source surface, capture generation, dimensions, crop, rotation, logical-to-physical coordinate transform, display scale and capture time. Multi-monitor offsets, negative coordinates, fractional scaling, window movement and resizing must be represented explicitly. A click must reference the observation/coordinate frame it was planned against.

Do not rely on OCR as the primary selector when an API or accessibility tree is available. OCR and vision guesses must carry lower confidence and be confirmed before consequential actions. A black, protected, stale or occluded capture is not a usable observation simply because an image file exists.

### 7.2 Model-neutral execution

Use Vesper's existing image content parts and pre-dispatch capability checks. Semantic-only tasks should remain available to models that support tool use but not image input. Visual-only tasks must fail clearly or require an explicitly approved capable model; no hidden provider switch or undisclosed cloud upload is allowed.

A provider-native computer-use protocol may be supported by a leaf adapter later, but Bridge must also work through ordinary structured tool calls. Do not hard-code a model name, subscription tier or claimed game ability into core architecture. Model capability and authorization are runtime facts.

OpenAI's current computer-use guidance treats on-screen and third-party content as untrusted relative to user intent and calls for care around sensitive actions. Bridge adopts that trust distinction independently of provider selection. [^S23]

### 7.3 Bounded data handling

Separate control messages from bulky artifacts. Initially remain within existing MCP response limits, including base64 expansion and metadata overhead. If an image cannot fit, request a smaller relevant crop, reduce resolution where usable, use semantic state, or report that the observation cannot be transferred safely. Do not silently enlarge global transport limits.

A later binary/artifact transfer route needs its own byte, pixel, decompression, lifetime and access controls. Local artifact references must be resolved to permitted content by the host before provider dispatch; a remote provider cannot read an arbitrary local file reference.

Default behavior retains only a small working set of observations. Screenshots and full accessibility dumps are not automatically added to durable memory or learning. Any retained media must follow an explicit storage policy with deletion controls and must be redacted before transmission, not only before logging.

## 8. Execution, concurrency and recovery

### 8.1 State and ownership

The session states are `discovered`, `connecting`, `ready`, `paused`, `recovering`, `quarantined` and `closed`. An individual operation has a separate state machine: `proposed`, `authorized`, `dispatched`, `applied`, `verifying`, then `succeeded`, `partial`, `failed`, `cancelled` or `unknown_outcome`.

A pending approval is not authorization. A dispatched request is not an applied edit. A completed application job is not a verified deliverable. Only a verified postcondition, or an explicitly defined application acknowledgment for an acknowledgment-only task, permits a corresponding success result.

Keep one owner for each application session and serialize state transitions. Do not hold shared metadata locks while awaiting a driver, rendering work, user consent or model inference. Long operations must keep their resource reservations while settling, including after the caller stops observing them.

### 8.2 Leases and sub-agents

Use resource-scoped exclusive mutation leases. A project or document cannot have two conflicting writers. Foreground input also requires a desktop/seat-wide lease because the operating system's active focus may be shared across applications.

Read-only work may run concurrently only when the adapter establishes that it is genuinely read-only. Verified semantic background actions can later permit more concurrency, but initial generic UI execution should remain serialized. Sub-agents receive restricted operations and resource references, not unrestricted driver connections.

A lease includes a fencing generation and expiry. The supervised worker must reject stale generations before dispatch. For third-party servers that cannot enforce fencing or stop an already queued mutation, quarantine the route until the request settles and state is reconciled. Do not claim distributed exactly-once execution.

### 8.3 Idempotency and reconciliation

Categorize operations as read-only, naturally idempotent, conditionally idempotent or non-idempotent. Prefer setting an absolute property over toggling it. Give created resources task-linked identities where the application supports them.

Write an intent record before dispatch and retain the resulting evidence after settlement. Local request deduplication protects only requests known to that owner; it does not prevent an external server from applying a timed-out request. After a disconnect during “create timeline,” search for the task-linked timeline and compare its properties before issuing another create.

Never automatically retry destructive or non-idempotent operations merely because the transport returned an error. Where state cannot be established, return `unknown_outcome`, retain relevant locks and ask for human inspection through the existing approval interface.

### 8.4 Stop, cancellation and rollback

Stop must be reachable without waiting for a model turn. It closes admission, signals owned work, revokes future action leases and requests release of driver-owned held keys/buttons. A local watchdog handles an unresponsive planner. Emergency release of previously owned inputs uses a narrowly scoped cleanup path even after ordinary action leases are revoked; revocation must not prevent release. The driver must distinguish inputs it owns from keys physically held by the person.

Cancellation does not imply rollback. A render may continue after the requesting connection closes; an already committed edit may remain. Report “cancellation requested; application job still running” until the application confirms otherwise. A bridge disconnect must not kill a user-owned application or unrelated process tree.

Rollback is adapter-specific compensation. A working-project backup, duplicate timeline and inverse edit are different recovery mechanisms with different guarantees. When restoration cannot be proven, preserve evidence and quarantine the session rather than displaying “restored” optimistically.

## 9. Security and privacy requirements

### 9.1 Authority boundaries

The model, skills, application content and third-party MCP servers supply data and proposals. Vesper's policy and approved host services supply authority. An application's document, web page, subtitle or error dialog cannot authorize a new tool, credential disclosure, software installation or filesystem scope.

Distinguish observe, edit-working-copy, export-new-file, destructive change, external transmission, software installation and account/system changes. Defaults should deny the latter categories unless explicitly approved for a concrete operation. Approval must bind the canonical operation, target, material arguments, output destination, schema version and relevant state. Changed material facts require reevaluation.

Authorization should be checked both at the public Bridge executor and at the final adapter dispatch boundary. Block equivalent backdoors through raw MCP gateways, unrestricted worker registries or browser script tools. Hiding a dangerous schema from the model is not access control.

### 9.2 Host-attached versus isolated execution

**Host-attached mode is not a strong sandbox.** An existing desktop application may already possess broad filesystem, network, account and plugin privileges. Restricting the arguments Bridge supplies does not remove the application's ambient authority. Approval must explain that distinction and name the application and scope of delegated control.

For stronger confinement, run the target application in a dedicated account, VM or other validated environment with separately constrained files, networking, clipboard and devices. Do not call a container isolated if it exposes the host display, home directory or unrestricted device interfaces. GPU-heavy Resolve workloads require separate performance and compatibility validation before promising a virtualized experience.

If a task requires a security boundary the current environment cannot provide, fail closed or offer a supervised manual workflow. Do not silently downgrade to host-attached control.

### 9.3 Driver and adapter controls

Sidecars run with the minimum practical host authority, a sanitized environment, bounded output and explicit executable identity. Do not use ambient `PATH` resolution for a trusted driver after initial configuration. Verify package signatures or authenticated provenance and digests, and record version/identity in evidence.

An executable signature does not prove benign behavior. Signing establishes origin and integrity; an adapter still requires code review, declared permissions, tests and update governance. Retain the existing data-only plugin rule unless a separately approved architecture decision intentionally changes it.

MCP browser servers can expose arbitrary server-side JavaScript; the inspected Playwright documentation labels such a tool unsafe. Bridge must not include that capability in the ordinary browser action allowlist. [^S29]

### 9.4 Content and egress controls

Treat accessibility names, filenames, media metadata, transcripts, web text, images and tool descriptions as untrusted. Delimit them from instructions; never convert their embedded requests into authority. Test indirect prompt injection both in visible text and in tool results.

Grant access independently for local processing, model-provider transmission, remote driver transmission and durable storage. Use selected-window capture where possible and redact excluded regions before any outbound request. Clipboard access is separate from keyboard input and is off by default.

Credentials stay in existing credential-management paths. Do not expose API keys, browser cookies, bearer tokens or portal restore tokens in model context or logs. User-assisted login should return control to the person; capturing a password field is not a required part of automation.

### 9.5 Supply chain and update policy

Pin executable releases and their transitive runtime requirements in a reviewed adapter lockfile. Record upstream repository, release/tag or commit, digest, supported OS/architecture, license notices, required interpreter and required permissions. Keep Cua, community Resolve code and benchmark assets distinguishable in the inventory.

Do not infer uniform licensing for an entire dependency tree from a repository badge. ViZDoom explicitly distinguishes its original code from engine dependencies, and the same review discipline applies to media tools, codecs, models and sample assets. [^S25]

## 10. DaVinci Resolve reference adapter

### 10.1 Connection decision

The adapter's diagnostic flow must identify the installed Resolve edition/build, platform, scripting availability, documented vendor-native connection options and active project. The Phase 0 evidence package must retain the relevant vendor manual or installed developer-document version and the tool/schema inventory observed on that machine.

Prefer the vendor-maintained integration when it supports the required task with acceptable authority and verification. Otherwise use documented Python/Lua scripting through a minimal, supervised sidecar. A community MCP server is a candidate implementation detail, not an architectural dependency or proof that every exposed command is safe.

Do not use direct SQL against Resolve project databases, undocumented project-file rewriting or an advanced community “edit the database” mode in the initial adapter. Those approaches need a separate compatibility and corruption-risk evaluation. Missing effect support is a capability limitation, not permission to mutate private formats.

### 10.2 Initial supported editing subset

Target media inspection/import, bins or collections, creation of a working project/timeline, clip placement and supported trims, titles through a validated template, supported audio adjustments, approved look/preset application, draft rendering and export inspection. Each operation is included only after the chosen interface demonstrates it on the certified edition/build.

Stabilization, denoising, retiming, complex Fusion graphs, advanced color operations, subtitles and third-party effects are optional capabilities, not blanket promises. Report installed effect availability and parameter support before planning with them. “Improve quality” must be translated into an explicit operation and review criterion; it is not a claim that lost detail can be recovered.

### 10.3 Non-destructive task lifecycle

**Preflight.** Confirm the source file set, readable paths, media fingerprints, duration/stream information, output location and free-space budget. Record whether media is variable frame rate, rotated, HDR or accompanied by external audio. Obtain an editorial brief or state conservative defaults that can be reviewed.

**Plan.** Represent the edit as a structured sequence of source ranges, timeline positions, title/effect selections, audio intent and output settings. Use rational frame/time representations and explicit rates, not floating-point seconds alone. Map variable-rate source timestamps deliberately to the chosen timeline rate.

**Working copy.** Create a new project or a verified duplicate and record the original project/media as protected inputs. Require a documented backup/export or equivalent checkpoint before modifying an existing project. Saving a Bridge JSON plan is not an application backup.

**Execute.** Import only approved media, apply a bounded batch, re-read state and verify the intended timeline. Treat missing media, mismatched rates, unavailable effects and inconsistent selection as blockers. On failure, preserve the working copy and precise partial-state report.

**Review.** Render a draft or representative previews with timecoded change notes. Technical validation and aesthetic review are separate: an output can be technically valid and still require a human editorial decision. A model's self-rating is not independent quality assurance.

**Deliver.** Export to a new destination, wait for the actual job to settle and inspect the resulting artifact. Retain a concise manifest that links source identities, working project, edit-plan revision, render settings, output hash and verification evidence.

### 10.4 Verification contract

Mandatory technical checks include file existence and readability, expected container/streams, dimensions, intended rate, duration tolerance, required audio, render status and absence of known missing-media errors. ffprobe supplies machine-readable format/stream information, but that alone does not prove complete decode integrity or visual quality. [^S28]

Add bounded decode checks, selected frame/audio inspection and timeline readback. For the certified reference export, use a full decode when practical and document any sample-only verification. Check synchronization and frame-boundary expectations using the reference fixture, not a generic duration-only test.

Objective assertions should be task-specific: the title is present in the intended range; a removed source interval is absent; an audio track exists and meets an explicitly chosen level criterion; a requested transition is present where supported. A black-frame detector must not reject an intentional fade without considering the plan.

OpenTimelineIO may be introduced as an interchange representation, but Bridge must preserve an explicit unsupported-feature ledger for its own adapter. Do not promise lossless round-tripping of every Resolve effect, node or application-specific setting merely because a timeline can be exported. [^S27]

## 11. Desktop and browser support strategy

| Environment | Initial product posture | Required certification evidence |
|---|---|---|
| Windows desktop | Candidate generic-control lane; no implicit administrator elevation | Exact app/window binding, UIA/visual actions, integrity-boundary refusal, DPI and focus tests |
| macOS desktop | Candidate lane with explicit system permissions and stable executable identity | Accessibility/capture permissions, relaunch behavior, app targeting and correct denial/revocation handling |
| Linux X11/XWayland | Candidate lane, dependent on the actual target application's window system | Capture, AT-SPI/native input, target delivery and synthetic-event rejection tests |
| Linux GNOME/Mutter | Capability-gated lane, with documented setup prerequisites | Compositor-specific geometry, portal consent, foreground input and no unsafe fallback |
| Linux KDE/KWin | Experimental until the selected driver and app pass a complete matrix | Refuse unsupported focus/activation routes; no claim of stock-Wayland parity |
| Dedicated browser profile | Preferred browser test environment | Profile separation, exact tab binding, file/network policy and clean shutdown |
| Existing user browser profile | Elevated-risk, explicit opt-in only | Account-data disclosure warning, binding evidence and no silent profile copying |

The desktop posture above follows the inspected Cua matrix; it is not a claim that Vesper has passed these lanes. XDG's Remote Desktop portal requires session setup and user-granted devices, with supported input routes including EIS/libei. Required system consent must remain visible and must not be bypassed by switching control techniques. [^S20]

Use the existing Playwright route for ordinary web applications when its semantic controls meet the task. A new browser driver should be justified by a concrete gap, such as exact native application-window integration, rather than duplicating browser automation already hosted by Vesper.

## 12. Game-control extension

The game example is valid as an extension of Bridge's observe/act contract, but it introduces different latency and control requirements. The first proof should use an offline reference environment with documented APIs and reproducible episodes. ViZDoom is a suitable candidate for that experiment; its published capabilities do not imply that Counter-Strike exposes an equivalent interface. [^S25]

Separate a slower task/planning loop from a faster local controller. The model can select bounded actions or goals; an approved local controller can execute short input sequences, enforce timeouts and release controls. Measure end-to-end perception-to-action latency before selecting a control rate. A fast driver cannot remove remote model inference latency.

The initial game contract includes attach, observe, bounded input duration, pause where the environment supports it, reset in a disposable test environment, episode outcome and emergency release. A long held key must have a maximum duration even if the model never returns.

Counter-Strike remains an application-specific research target. Before enabling it, establish the exact installed title/build, documented permissible mode, available observation/control route and account/anti-cheat implications. No public competitive automation, stealth input, anti-cheat bypass, memory injection or unsupported internal-state extraction is part of this PRD. Failure to validate those conditions leaves that adapter unavailable while other Bridge applications remain useful.

## 13. User experience and workflow learning

### 13.1 Interaction model

Natural language is the primary interface. A person should not need to manually choose “API versus accessibility versus vision” for every action. Vesper should propose the strongest supported route and expose its consequences: target application, working copy, observation scope, provider destination, foreground/background behavior and output location.

Proposed commands are `/bridge`, `/bridge discover`, `/bridge doctor`, `/bridge connect`, `/bridge status`, `/bridge pause`, `/bridge resume` and `/bridge disconnect`. These are new product commands, not current functionality. The always-available stop control must be discoverable independently of those commands and must not require the target application's focus.

The TUI should show the application and connection state, current action, whether input is foreground or verified background, last verified checkpoint, unresolved risk and asynchronous job state. Use existing activity/output conventions rather than a new dashboard. A percentage may be displayed only when based on actual job progress or a defined task model, not elapsed-time guesswork.

ACP receives equivalent structured progress, approvals, artifacts and terminal outcomes. Existing VesperLens review can present previews or questions, but a browser review surface must not become a dependency for stop, denial or core execution.

### 13.2 Failure language

Prefer specific outcomes: “Resolve was found, but the installed edition does not expose the requested operation”; “the window moved; a fresh observation is required”; “the export request may have been accepted; status reconciliation is in progress.” Avoid “connected successfully” when only a port was reached or “done” when a render was merely queued.

### 13.3 Learning policy

A verified workflow may be proposed for reuse through the existing learning mechanism. Persist the procedural template, required capability versions, typed parameters, validation rules and known failure conditions. Do not persist access tokens, raw desktop captures or authority that outlives the original task.

Revalidate a learned workflow when the application, adapter, schema or material environment changes. Learning may improve route choice and reduce repeated planning; it may never increase permission automatically. Failed or incomplete runs may contribute bounded failure knowledge, but must not be saved as proven success workflows.

## 14. Non-functional requirements and budgets

These are **proposed release targets**, not measured performance claims. Phase 0 establishes a reproducible baseline; changing a target requires a recorded decision, not retrospective relabeling of a failed test.

| ID | Target / invariant | Measurement boundary |
|---|---|---|
| NF-01 Disabled-path cost | No sidecar, capture, Bridge network activity or model call when disabled | Process, filesystem and network observations during both host startup paths |
| NF-02 Stop admission | Stop prevents new Bridge dispatch within 250 ms at p95 | Local service receiving stop to admission closure; excludes already committed app effects |
| NF-03 Held-input release | On a responsive certified driver, release owned input within 1 second at p95 | Local stop/watchdog event to verified release; failure must quarantine and warn |
| NF-04 Core overhead | Pure authorization/routing/journaling overhead below 100 ms at p95 | Excludes OS capture, driver IPC, application processing, storage flush variance and model latency; disclose each separately |
| NF-05 Bounded work | One in-flight mutation per conflicting resource; no unbounded queues | Stress and cancellation tests with a slow or disconnected consumer |
| NF-06 Observation bounds | Preserve eight-image tool-result maximum and existing transport response caps | Encoded response size including base64/metadata; reject before allocation/dispatch beyond negotiated budgets |
| NF-07 Decode safety | Initial proposal: maximum 16 million decoded pixels and 64 MiB pixel storage per observation package | Combined package, not per image; enforce before decoder allocation wherever supported |
| NF-08 Working capture queue | At most two pending observation packages; replace superseded observations, never silently drop action outcomes | Slow-model and high-capture-rate tests |
| NF-09 Deadlines | Explicit per-operation deadline; proposed ordinary-action default 15 seconds | Renders use jobs with separately defined deadlines, not a global timeout increase |
| NF-10 Retry bound | At most two safe transport retries per operation; no automatic ambiguous mutation retry | Injected disconnect and timeout traces |
| NF-11 Data retention | No continuous recording or durable screenshot retention by default | Storage audit after ordinary completion, cancellation and crash |
| NF-12 Compatibility | Current MSRV and architecture gates continue to pass | Supported CI targets; no hidden interpreter requirement on disabled path |
| NF-13 Truthfulness | No known failed, interrupted or unverified operation reported as verified success | Deterministic outcome-arbitration and end-to-end tests |
| NF-14 Availability claims | Certification is per application/build/edition/OS/driver/route | Published support manifest linked to evidence and date |

Record p50, p95, sample size, warm/cold state, hardware, OS, driver/application versions and failure counts. Show model inference, capture, IPC, application execution, verification and rendering separately. Cost metrics should identify provider-reported usage versus local estimates; never imply measured billing where none is available.

For the first generality release, a proposed task-level gate is at least 27 verified successes in 30 predefined non-destructive task runs across the two reference applications, with failures and human interventions disclosed. This is a bounded release check, not a claim of universal reliability or a statistically precise long-run success rate. All safety-critical tests remain mandatory regardless of that success fraction.

## 15. Verification and acceptance matrix

Use deterministic contract/process tests before live application tests. A fake driver proves protocol and state behavior only; it cannot certify actual UI delivery. Run live tests on disposable projects and explicitly approved local environments. Unavailable hardware or software makes the corresponding lane **not certified**, not “passed by skip.”

| Test | Scenario and required evidence | Requirements |
|---|---|---|
| AT-01 | Bridge disabled in both hosts: no new process, capture, network or provider action | BR-30, NF-01 |
| AT-02 | Ambiguous application names/windows: no attachment until exact selection | BR-01, BR-02 |
| AT-03 | PID/window reuse after restart: previous binding and lease rejected | BR-02, BR-15 |
| AT-04 | Missing license/edition feature, app or effect: clear capability refusal, no workaround | BR-03, BR-25 |
| AT-05 | Unknown schema/operation/argument: reject before driver dispatch | BR-03, BR-11 |
| AT-06 | Plan/read-only mode attempts launch, click, edit or export: zero mutation dispatch | BR-07 |
| AT-07 | User denial or closed approval channel: no dispatch through native or fallback routes | BR-08 |
| AT-08 | Approval arguments/target/schema changed after consent: approval invalidated | BR-08, BR-28 |
| AT-09 | Prompt injection in document, screenshot text, subtitle and tool result: authority unchanged | BR-05, BR-08, BR-22 |
| AT-10 | Raw MCP/prefix gateway and worker attempt to bypass Bridge: effective denial | BR-08, BR-14 |
| AT-11 | Text-only model with image observation: typed capability outcome, no silent image loss | BR-09, BR-10 |
| AT-12 | Oversized/compressed image and malformed media: bounded allocation and explicit failure | BR-09, NF-06, NF-07 |
| AT-13 | Multi-monitor/DPI/crop/rotation changes: correct coordinates or stale-observation refusal | BR-09, BR-15 |
| AT-14 | Black/protected/stale capture: no action claimed to be visually grounded | BR-09, BR-12 |
| AT-15 | Native API says success but project state unchanged: verification fails | BR-11, BR-12 |
| AT-16 | Two writers target one project; two UI sessions target one seat: exclusivity proven | BR-14, NF-05 |
| AT-17 | Safe background operation: prove intended state and no foreground-input leakage | BR-14, BR-27 |
| AT-18 | Duplicate request ID before/after acknowledgment: one accepted mutation | BR-16 |
| AT-19 | Disconnect after create request: reconciliation avoids duplicate creation | BR-16, BR-18 |
| AT-20 | Unknown queued mutation in third-party server: quarantine blocks new writes | BR-16, BR-18 |
| AT-21 | Human stop during slow model response: admission closes independently | BR-17, NF-02 |
| AT-22 | Stop/crash while holding key/button: release or explicit unsafe-state warning | BR-17, NF-03 |
| AT-23 | Render continues after cancellation: UI reports outstanding job, not rollback | BR-13, BR-17 |
| AT-24 | Driver crash/caller drop: owned cleanup settles; user app is not indiscriminately killed | BR-04, BR-18 |
| AT-25 | Restore backup/compensation failure: original evidence retained, session quarantined | BR-18, BR-19 |
| AT-26 | Source media and original project invariance across reference workflow | BR-19 |
| AT-27 | Export file missing, truncated, wrong duration/rate or missing audio: no success | BR-12, BR-20 |
| AT-28 | Reference edits/title/audio verified in real timeline and rendered samples | BR-12, BR-20 |
| AT-29 | Disk full, revoked file access, removed media or changed input during execution | BR-15, BR-19, BR-25 |
| AT-30 | Unsigned/tampered profile, untrusted sidecar and changed executable digest: refusal | BR-23, BR-28 |
| AT-31 | Secrets/private canaries absent from provider egress and durable logs | BR-22, BR-24 |
| AT-32 | OS permission revoke, portal replacement and unsupported KWin route: safe refusal | BR-03, BR-18 |
| AT-33 | Legacy and current MCP peer fixtures: negotiated behavior; no implicit security downgrade | BR-04, BR-28 |
| AT-34 | Same task through TUI and ACP: equivalent approvals, result and failure semantics | BR-21, BR-29 |
| AT-35 | App/adapter/schema upgrade invalidates stale workflows and approvals | BR-26, BR-28 |
| AT-36 | Second unrelated app uses the same core contracts without new core app branches | BR-27 |
| AT-37 | Slow consumers, repeated reconnects and task cancellation: bounded memory/process count | BR-04, BR-18, NF-05 |
| AT-38 | Finite offline game input and episode stop: no stuck input or hidden network play | BR-17; game phase |
| AT-39 | Required isolation unavailable: no downgrade to unsandboxed host execution | BR-08, BR-22 |
| AT-40 | Inspect/resume old session: audit replay only, no automatic app mutation | BR-18, BR-24 |
| AT-41 | Requested plan needs missing capabilities: block or disclose a reduced plan with changed acceptance criteria | BR-06 |
| AT-42 | Automatic skill routing and learning: permissions unchanged; only verified runs become proven workflow templates | BR-05, BR-26 |
| AT-43 | Keyboard-only/terminal-readable use of consent, stop and diagnostics; no color-only essential state | BR-29 |
| AT-44 | Multiple model turns reuse one owned application session; worker replacement revalidates grants and target identity | BR-04 |

The current OSWorld 2.0 repository recommends the `osworld-v2-2026.08.08` release and warns against mixing code, tasks, assets and site versions. Use it as a separate, version-pinned evaluation suite; report its results independently from Bridge's own safety and media tests. No upstream benchmark score transfers automatically to Vesper. [^S26]

## 16. Implementation phases and release gates

### Phase 0 — Reconnaissance and adapter decision

Inspect the current checkout, owning crate contracts, tool media path, permissions, MCP call lifetimes, host parity and architecture allowlist. Compare against the pinned baseline without assuming the checkout is unchanged. Capture the installed Resolve documentation, edition/build and supported operations. Evaluate Cua on the actual intended compositor/OS, including denial cases. Select one production candidate route and one reference application fixture.

Deliver an evidence index, compatibility matrix, threat model, dependency/license inventory, integration map and proposed ADRs. **Gate:** no core implementation proceeds on invented APIs, unknown executable provenance or an unproven Resolve bootstrap. Pure contract work may proceed while a platform lane remains explicitly blocked.

### Phase 1 — Pure contracts and deterministic reference driver

Implement `vesper-bridge` types, state transitions, capability/identity model, result classification, leases, deadlines and fake driver. Define schema-version compatibility and a minimal journal/storage port. Build deterministic tests for denial, stale identity, duplicate requests, unknown outcomes and shutdown.

**Gate:** mandatory contract tests and current architecture/MSRV gates pass; no production app-control side effects exist yet. Test fixtures are clearly labeled and cannot masquerade as real application evidence.

### Phase 2 — Shared Vesper host integration

Compose the optional service in `vesper-harness`; expose the minimal tool surface through existing executor and image paths. Add consent, diagnostics, progress and independent stop to TUI and ACP. Reuse existing transport functionality; implement owned long-lived connections only where needed and tested.

**Gate:** AT-01 through the applicable policy/provider/lifecycle tests pass through real host process paths. A default-off build and Bridge-disabled runtime remain behaviorally compatible.

### Phase 3 — Resolve vertical slice

Implement the validated connection and narrow non-destructive workflow: inspect/import, working timeline, a small supported edit set, draft render, actual artifact validation and partial-failure reporting. Use a fixed media fixture with known frame/audio/title expectations.

**Gate:** real application readback and rendered evidence, original-input invariance, successful cancellation/reconciliation tests and a named certified version/edition/platform. A model-written completion report is insufficient.

### Phase 4 — Generic driver and unrelated application

Add Cua or the selected equivalent behind the Bridge port, semantic observation and tightly bounded visual fallback. Complete an unrelated app workflow. Reuse existing Playwright where it is the appropriate route. Validate the selected OS lane rather than promising cross-platform parity.

**Gate:** AT-36 passes; no application-specific branching has entered the pure core; relevant focus, geometry, stop, permission and prompt-injection tests pass on the real driver.

### Phase 5 — Hardening and generality release

Complete support manifests, upgrade invalidation, recovery, opt-in evidence retention, budget telemetry and reviewed workflow learning. Run the full mandatory matrix for each advertised lane, the predefined two-app task suite and an optional pinned OSWorld subset. Audit packaging and uninstall behavior without altering unrelated state.

**Gate:** accepted security review, published known limitations, certified support matrix, no unresolved critical data-loss or authority-bypass defect, and explicit approval to release. Source-code completion does not authorize replacing a person's installed application.

### Phase 6 — Experimental games and remote workers

Develop the controlled game loop and optional remote worker transport behind separate flags. Validate input timing, watchdog behavior, episode reset, remote identity and media egress. Counter-Strike is considered only after its specific feasibility and permitted-mode checks pass.

**Gate:** independent experimental evidence and clear labeling. Neither game nor remote-worker work may weaken local desktop safety or delay a useful media/application release.

## 17. Risks, decisions and unresolved items

| Risk | Impact | Mitigation and release consequence |
|---|---|---|
| App interface changes or incomplete official docs | Wrong operations or unstable adapter | Pin version/edition; retain interface evidence; reject unknown capability generations |
| Host desktop has broad ambient authority | Data exposure or unintended external effects | Explain host-attached risk; narrow typed operations; require validated isolation where demanded |
| Focus/geometry race | Input reaches wrong window | Exact binding, fresh observations, input lease and structured refusal; fail the affected lane |
| Timeout after committed action | Duplicate timelines, edits or exports | Intent journal and reconciliation; no blind replay |
| Driver/app cannot be stopped reliably | Stuck input or continued destructive work | Independent watchdog, held-input bounds, settlement reporting and quarantine |
| Native Resolve route lacks requested effects | Incomplete editing promise | Publish the exact subset; preview alternatives; no undocumented database editing |
| Overly broad third-party tool surface | Hidden arbitrary execution | Host allowlist per action; block raw gateway bypasses; separate reviewed advanced mode |
| Context/image growth | Cost, latency or memory exhaustion | Bounded observations, semantic-first selection, artifact lifecycle and explicit budgets |
| Learned workflow becomes stale | Repeated incorrect actions | Version binding, preconditions and re-verification before reuse |
| Dependency/toolchain expansion | Build and packaging regressions | Minimal new crate, optional sidecars, pinned artifacts, unchanged disabled path |

Required architecture decisions are: AD-01 native Bridge service with data-only plugin profiles; AD-02 hybrid application-control routes; AD-03 explicit host-attached versus isolated modes; AD-04 owned sessions and exclusive mutation leases; AD-05 independent stop and unknown-outcome handling; AD-06 existing image/tool reuse; AD-07 versioned MCP interoperability without forced SDK replacement; and AD-08 non-destructive Resolve adapter without private database mutation.

Open items have owners and defaults. The implementation lead owns exact Resolve connector selection and verified SDK signatures. The platform owner owns the first certified OS lane. The security reviewer owns approval scopes and sandbox claims. The maintainer owns driver dependency pins and licensing records. The default is **unavailable/not certified** for unresolved capabilities, not a guessed implementation.

The current user's installed application editions, target machine, preferred model and acceptable screenshot-egress policy are not assumed by this PRD. The implementation should resolve them through discovery and explicit setup, not hard-coded personal configuration.

## 18. Definition of done and handoff

Bridge is ready for its first generality release when two unrelated application workflows operate through the same core, all mandatory safety tests pass on every advertised lane, Resolve produces a verified non-destructive reference export, TUI and ACP have equivalent authority semantics, stop works independently of model progress, and uncertainty is represented without false success.

The handoff must include source and schema changes, approved ADRs, dependency pins, tests and raw evidence locations, the support matrix, known limitations, migration/uninstall behavior and a clear distinction between simulated, process-level and live-application results. Test counts alone are not acceptance; each claim must map to a required behavior and the environment in which it was observed.

The companion implementation prompt directs the coding agent to begin with Phase 0, preserve current repository contracts, and implement only the authorized phase. It does not authorize production deployment, account changes, software purchases or uncontrolled desktop access.

## 19. Source register

Sources are cited through numbered footnotes. The register below preserves original titles, publishers, versions and access limitations. Repository contracts describe intended ownership; the specifically inspected executor implementation establishes only the behavior visible in that code, not a full runtime audit. Rolling documentation must be pinned again when selecting executable dependencies.

**S01 — Agent Vesper.** [Workspace manifest (Cargo.toml)](https://github.com/99percentgrip/agent-vesper/blob/75c1a5508055f0a373f9ca335b64c414dc97a393/Cargo.toml). Commit 75c1a5508055f0a373f9ca335b64c414dc97a393; 12 September 2026 UTC. Verified version, MSRV, workspace packages and license.

**S02 — Agent Vesper.** [Production crate contracts](https://github.com/99percentgrip/agent-vesper/blob/75c1a5508055f0a373f9ca335b64c414dc97a393/crates/AGENTS.md). Same pinned commit. Current ownership, dependency and sandbox boundaries; contracts, not proof of every runtime claim.

**S03 — Agent Vesper.** [MCP client and signed-plugin contracts](https://github.com/99percentgrip/agent-vesper/blob/75c1a5508055f0a373f9ca335b64c414dc97a393/crates/vesper-mcp/AGENTS.md). Same pinned commit. Declarative-only plugins, signed release loading, existing stdio/HTTP and bounds.

**S04 — Agent Vesper.** [Tool executor implementation](https://github.com/99percentgrip/agent-vesper/blob/75c1a5508055f0a373f9ca335b64c414dc97a393/crates/vesper-agent/src/executor.rs). Same pinned commit; lines 1–180 inspected. Actual ToolContext, ToolResult.media, injected_tools and eight-image limit.

**S05 — Agent Vesper.** [Agent loop and permission contracts](https://github.com/99percentgrip/agent-vesper/blob/75c1a5508055f0a373f9ca335b64c414dc97a393/crates/vesper-agent/AGENTS.md). Same pinned commit; opening 180 lines inspected. Shared loop, capability gating, tool registration, cancellation and workflow learning.

**S06 — Agent Vesper.** [Shared hosted services contracts](https://github.com/99percentgrip/agent-vesper/blob/75c1a5508055f0a373f9ca335b64c414dc97a393/crates/vesper-harness/AGENTS.md). Same pinned commit; opening 130 lines inspected. Composition seam, shared Lens tools, hosted MCP/browser tools and owned worker cleanup.

**S07 — Cua.** [Cua Driver README](https://github.com/trycua/cua/blob/main/libs/cua-driver/README.md). Rolling main documentation, accessed 13 September 2026. Rust driver, CLI/MCP and SDK integration, runtime permission modes, macOS identity; exact release pin remains an implementation gate.

**S08 — Cua.** [Driver platform support](https://github.com/trycua/cua/blob/main/docs/content/docs/reference/cua-driver/platform-support.mdx). Rolling main documentation, accessed 13 September 2026. Windows/macOS/X11 behavior, compositor-specific Wayland limits, experimental KWin and nested compositor.

**S09 — Cua.** [Driver limits and Linux completion plan](https://github.com/trycua/cua/blob/main/libs/cua-driver/docs/linux-support-completion-plan.md). Rolling main, documentation also retrieved through Context7. Nested compositor results cannot establish capabilities on a stock compositor.

**S10 — Microsoft.** [UFO: desktop and multi-device agent architecture](https://github.com/microsoft/UFO). Current repository accessed 13 September 2026. MIT project; hybrid GUI/API desktop agent and separate multi-device orchestration. No benchmark transfer to Vesper assumed.

**S11 — Microsoft.** [Playwright MCP README and security notice](https://github.com/microsoft/playwright-mcp). Rolling documentation accessed 13 September 2026. Accessibility-oriented browser automation, profile options, and explicit absence of a security boundary.

**S12 — Model Context Protocol.** [Protocol specification](https://modelcontextprotocol.io/specification/2026-07-28). Revision 2026-07-28. Current dated specification located during research.

**S13 — Model Context Protocol.** [Transport overview](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports). Revision 2026-07-28. Request metadata, stdio/Streamable HTTP, cancellation and legacy interoperability.

**S14 — Model Context Protocol.** [Tools specification](https://modelcontextprotocol.io/specification/2026-07-28/server/tools). Revision 2026-07-28. Discovery, schemas, structured tool content and tool annotations.

**S15 — Model Context Protocol.** [Understanding authorization in MCP](https://modelcontextprotocol.io/docs/2026-07-28/tutorials/security/authorization). Revision 2026-07-28. Transport authorization is distinct from application-level mutation permission.

**S16 — Model Context Protocol Rust SDK.** [Client service lifecycle implementation](https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/src/service/client.rs). Rolling main retrieved through Context7 on 13 September 2026. Initialize/Discover/Auto lifecycle concepts. A published package version and MSRV were not certified.

**S17 — Blackmagic Design.** [DaVinci Resolve Studio: scripting and integration](https://www.blackmagicdesign.com/products/davinciresolve/studio). Product documentation accessed 13 September 2026. Vendor-described Python/Lua scripting, remote scripting and workflow integration.

**S18 — Blackmagic Design; syndicated by Creative COW.** [Blackmagic Design Announces DaVinci Resolve 21.1](https://creativecow.net/blackmagic-design-announces-davinci-resolve-21-1/). 8 September 2026. Blackmagic-authored announcement available as indexed syndicated text. Confirms assistant integration; exact MCP setup, tool inventory and edition restrictions are not established by this source. Direct vendor release and full 21.1 manual could not be retrieved.

**S19 — Samuel Gursky and contributors.** [DaVinci Resolve MCP](https://github.com/samuelgursky/davinci-resolve-mcp). Rolling README accessed 13 September 2026. Community scripting adapter and version-specific reports; advanced DB/XML editing is excluded from Bridge baseline. Older free-edition reports do not certify 21.1.

**S20 — XDG Desktop Portal.** [Remote Desktop portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html). Current documentation, interface version 2. User consent, device grants, session lifecycle and libei/EIS input.

**S21 — XDG Desktop Portal.** [ScreenCast portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html). Current documentation accessed 13 September 2026. Capture metadata and version-dependent stream identity; node reuse caveat.

**S22 — Microsoft Learn.** [SendInput function](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput). API page accessed 13 September 2026. Integrity restrictions and distinction between submitted input and successful app behavior.

**S23 — OpenAI.** [Computer use guide](https://developers.openai.com/api/docs/guides/tools-computer-use). Current API documentation accessed 13 September 2026. Observation/action integration and guidance on untrusted screen content and sensitive actions. No model availability or subscription entitlement inferred.

**S24 — BAAI-Agents.** [Cradle: General Computer Control](https://github.com/BAAI-Agents/Cradle). Research project; 2024 work, repository accessed 13 September 2026. MIT research prior art for screen/input control and app/game-specific skills; not a current production SLA.

**S25 — Farama Foundation.** [ViZDoom](https://github.com/Farama-Foundation/ViZDoom). Repository accessed 13 September 2026. Visual research environment with sync/async modes. Original ViZDoom code is MIT; engine/assets have separate licensing.

**S26 — XLANG Lab.** [OSWorld 2.0](https://github.com/xlang-ai/OSWorld-V2). Recommended benchmark release osworld-v2-2026.08.08. Current long-horizon evaluation candidate. Pin code, tasks, assets and mocked sites to one release; do not compare mixed releases.

**S27 — OpenTimelineIO.** [File format specification](https://opentimelineio.readthedocs.io/en/latest/tutorials/otio-file-format-specification.html). Rolling 0.19.0.dev1 documentation accessed 13 September 2026. Optional timeline interchange reference; development documentation is not a production dependency pin.

**S28 — FFmpeg.** [ffprobe documentation](https://ffmpeg.org/ffprobe.html). Current documentation accessed 13 September 2026. Machine-readable stream/container inspection. Metadata checks are not proof of subjective quality or complete decode integrity.

**S29 — Microsoft Playwright MCP.** [Configuration and unsafe tool documentation](https://github.com/microsoft/playwright-mcp/blob/main/_autodocs/tools-tabs-and-console.md). Rolling main retrieved through Context7 on 13 September 2026. Arbitrary server-side JavaScript tool is explicitly unsafe; do not expose it as ordinary browser interaction.

**S30 — Agent Vesper.** [Documentation index and historical architecture warning](https://github.com/99percentgrip/agent-vesper/blob/75c1a5508055f0a373f9ca335b64c414dc97a393/docs/README.md). Same pinned commit as S01. Current code/crate contracts must not be replaced by obsolete Stage 5 assumptions.

[^S01]: Agent Vesper, [Workspace manifest (Cargo.toml)](https://github.com/99percentgrip/agent-vesper/blob/75c1a5508055f0a373f9ca335b64c414dc97a393/Cargo.toml). Commit 75c1a5508055f0a373f9ca335b64c414dc97a393; 12 September 2026 UTC. Source register S01.
[^S02]: Agent Vesper, [Production crate contracts](https://github.com/99percentgrip/agent-vesper/blob/75c1a5508055f0a373f9ca335b64c414dc97a393/crates/AGENTS.md). Same pinned commit. Source register S02.
[^S03]: Agent Vesper, [MCP client and signed-plugin contracts](https://github.com/99percentgrip/agent-vesper/blob/75c1a5508055f0a373f9ca335b64c414dc97a393/crates/vesper-mcp/AGENTS.md). Same pinned commit. Source register S03.
[^S04]: Agent Vesper, [Tool executor implementation](https://github.com/99percentgrip/agent-vesper/blob/75c1a5508055f0a373f9ca335b64c414dc97a393/crates/vesper-agent/src/executor.rs). Same pinned commit; lines 1–180 inspected. Source register S04.
[^S05]: Agent Vesper, [Agent loop and permission contracts](https://github.com/99percentgrip/agent-vesper/blob/75c1a5508055f0a373f9ca335b64c414dc97a393/crates/vesper-agent/AGENTS.md). Same pinned commit; opening 180 lines inspected. Source register S05.
[^S06]: Agent Vesper, [Shared hosted services contracts](https://github.com/99percentgrip/agent-vesper/blob/75c1a5508055f0a373f9ca335b64c414dc97a393/crates/vesper-harness/AGENTS.md). Same pinned commit; opening 130 lines inspected. Source register S06.
[^S07]: Cua, [Cua Driver README](https://github.com/trycua/cua/blob/main/libs/cua-driver/README.md). Rolling main documentation, accessed 13 September 2026. Source register S07.
[^S08]: Cua, [Driver platform support](https://github.com/trycua/cua/blob/main/docs/content/docs/reference/cua-driver/platform-support.mdx). Rolling main documentation, accessed 13 September 2026. Source register S08.
[^S09]: Cua, [Driver limits and Linux completion plan](https://github.com/trycua/cua/blob/main/libs/cua-driver/docs/linux-support-completion-plan.md). Rolling main, documentation also retrieved through Context7. Source register S09.
[^S10]: Microsoft, [UFO: desktop and multi-device agent architecture](https://github.com/microsoft/UFO). Current repository accessed 13 September 2026. Source register S10.
[^S11]: Microsoft, [Playwright MCP README and security notice](https://github.com/microsoft/playwright-mcp). Rolling documentation accessed 13 September 2026. Source register S11.
[^S12]: Model Context Protocol, [Protocol specification](https://modelcontextprotocol.io/specification/2026-07-28). Revision 2026-07-28. Source register S12.
[^S13]: Model Context Protocol, [Transport overview](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports). Revision 2026-07-28. Source register S13.
[^S14]: Model Context Protocol, [Tools specification](https://modelcontextprotocol.io/specification/2026-07-28/server/tools). Revision 2026-07-28. Source register S14.
[^S15]: Model Context Protocol, [Understanding authorization in MCP](https://modelcontextprotocol.io/docs/2026-07-28/tutorials/security/authorization). Revision 2026-07-28. Source register S15.
[^S16]: Model Context Protocol Rust SDK, [Client service lifecycle implementation](https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/src/service/client.rs). Rolling main retrieved through Context7 on 13 September 2026. Source register S16.
[^S17]: Blackmagic Design, [DaVinci Resolve Studio: scripting and integration](https://www.blackmagicdesign.com/products/davinciresolve/studio). Product documentation accessed 13 September 2026. Source register S17.
[^S18]: Blackmagic Design; syndicated by Creative COW, [Blackmagic Design Announces DaVinci Resolve 21.1](https://creativecow.net/blackmagic-design-announces-davinci-resolve-21-1/). 8 September 2026. Source register S18.
[^S19]: Samuel Gursky and contributors, [DaVinci Resolve MCP](https://github.com/samuelgursky/davinci-resolve-mcp). Rolling README accessed 13 September 2026. Source register S19.
[^S20]: XDG Desktop Portal, [Remote Desktop portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html). Current documentation, interface version 2. Source register S20.
[^S21]: XDG Desktop Portal, [ScreenCast portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html). Current documentation accessed 13 September 2026. Source register S21.
[^S22]: Microsoft Learn, [SendInput function](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput). API page accessed 13 September 2026. Source register S22.
[^S23]: OpenAI, [Computer use guide](https://developers.openai.com/api/docs/guides/tools-computer-use). Current API documentation accessed 13 September 2026. Source register S23.
[^S24]: BAAI-Agents, [Cradle: General Computer Control](https://github.com/BAAI-Agents/Cradle). Research project; 2024 work, repository accessed 13 September 2026. Source register S24.
[^S25]: Farama Foundation, [ViZDoom](https://github.com/Farama-Foundation/ViZDoom). Repository accessed 13 September 2026. Source register S25.
[^S26]: XLANG Lab, [OSWorld 2.0](https://github.com/xlang-ai/OSWorld-V2). Recommended benchmark release osworld-v2-2026.08.08. Source register S26.
[^S27]: OpenTimelineIO, [File format specification](https://opentimelineio.readthedocs.io/en/latest/tutorials/otio-file-format-specification.html). Rolling 0.19.0.dev1 documentation accessed 13 September 2026. Source register S27.
[^S28]: FFmpeg, [ffprobe documentation](https://ffmpeg.org/ffprobe.html). Current documentation accessed 13 September 2026. Source register S28.
[^S29]: Microsoft Playwright MCP, [Configuration and unsafe tool documentation](https://github.com/microsoft/playwright-mcp/blob/main/_autodocs/tools-tabs-and-console.md). Rolling main retrieved through Context7 on 13 September 2026. Source register S29.
[^S30]: Agent Vesper, [Documentation index and historical architecture warning](https://github.com/99percentgrip/agent-vesper/blob/75c1a5508055f0a373f9ca335b64c414dc97a393/docs/README.md). Same pinned commit as S01. Source register S30.
