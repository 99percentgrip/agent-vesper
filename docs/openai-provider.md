# Native OpenAI provider

OpenAI runs directly inside Vesper. Neither authentication mode installs,
bundles, launches, or reads credentials from Codex CLI or app-server.

## Activate in the TUI

1. Open `/settings` → Providers → OpenAI, then Save.
2. Choose API key (usage-based billing) or ChatGPT subscription, then Save.
3. For API mode, enter the key in the masked authentication screen. For
   subscription mode, open the displayed official verification page and enter
   the one-time code. Only approve a login you initiated. Esc cancels.
4. Save the provider preference and restart when prompted.

Use `/auth` to change authentication or sign out locally. Existing subscription
credentials are never silently replaced by an environment API key. Explicitly
selecting API mode enables API billing; `OPENAI_API_KEY` then overrides a stored
API key. Local sign-out disables that fallback until another explicit sign-in.

An eligible account, model entitlement, and permission to use device login are
required. Organization policy can disable device authorization. Subscription
access is not interchangeable with API credit. See the official
[authentication guidance](https://learn.chatgpt.com/docs/auth).

## Models and harness features

The verified capability catalog includes GPT-6 Astra, GPT-5.6 Sol, Terra and Luna,
GPT-5.5, GPT-5.4, GPT-5.2, GPT-5.3 Codex, and GPT-5.3 Codex Spark. Menus show
only entries also returned for your account. Fresh sessions use an available
model; `/model` changes the active model. `/thinking` offers low, medium, high, and xhigh; Astra and
the GPT-5.6 models also offer max. API mode additionally offers none for the
non-Codex models except Astra. Subscription mode does not advertise none.
Model switches repair incompatible selections. Unsupported combinations fail
before dispatch. Codex's ultra is a host-owned automatic delegation mode,
not a literal Responses effort, and is not advertised as one here. The shared
working-context budget is conservatively 272,000 tokens, except Spark at 128,000;
it is not a claim that the public models have only that capacity. Catalog
evidence: [GPT-5.4](https://developers.openai.com/api/docs/models/gpt-5.4) and
[GPT-6 Astra](https://developers.openai.com/api/docs/models/gpt-6-astra).

Both modes use Vesper's ordinary agent loop: confined file tools, shell tools,
permissions, plans and continuation, skills, memory, MCP/plugins, workers,
reasoning orchestration, and semantic compaction. Image input, streamed text
and reasoning summaries, encrypted reasoning continuation, function calls,
structured JSON, and token usage use the native Responses protocol.

Web tools remain controlled by Settings → Web tools, with the bundled driver;
see [web setup](web-tools.md). Changing provider does not grant network or tool
permissions. Legacy Z.ai-hosted search/vision MCP presets still need their own
service credentials; they do not become OpenAI-hosted tools.

Memory extraction uses native OpenAI when the host launches with OpenAI.
Embeddings use the independently configured source or existing local fallback,
not an invented subscription embeddings endpoint. In ACP, restart after a
footer provider swap if you also want to change the memory extraction provider.

## Available models

Vesper loads model choices for your selected account and authentication method.
API-key and ChatGPT subscription lists can differ. Subscription models hidden by
the service and models whose capabilities Vesper has not verified are excluded.
An older model may still appear in API mode if your account's API list includes it.

Open **Settings → Primary model** to choose a model. Reopening Settings refreshes
TUI choices; restart the ACP agent to refresh its editor menu. After changing
credentials, the old account list is discarded. If discovery fails, check sign-in
and connectivity, then choose **Retry model list** (Enter, R or click).
Esc returns to Settings. Vesper shows the failure reason without a guessed fallback list.

GPT-5.3 Codex Spark appears only when returned for your account. It accepts text,
uses a 128K context budget, and does not request reasoning summaries. Other supported
models retain their adapter-defined capabilities. Availability can change after
loading the menu, so a later service rejection is still possible.

## Usage and account limits

Run `/usage` for the shared aligned status panel: current model, reasoning,
permissions, estimated working context, and account usage. ChatGPT subscription
mode queries the native passive usage endpoint and reports returned primary,
weekly, and additional/premium windows with remaining percentages and reset
countdowns. Missing windows remain unknown, never unlimited. Repeat `/usage`
to refresh; it works independently of an active agent turn.

Switching to another provider and back reuses the valid selected OpenAI
credential. Use `/auth` only to rotate, replace, or sign out; provider
selection itself does not start another device login.

API-key mode does not have subscription windows. Its card identifies API billing
and links to the project/organization limits page; it does not invent a credit
balance or require an organization-admin key. GLM uses its native monitor through
the same status contract; providers without account endpoints still show the
model/context card with an explicit unavailable notice.

## ACP / editor usage

Sign in in the TUI first, or run:

```sh
agent-vesper-acp --provider openai --login
agent-vesper-acp --provider openai --check-auth
```

Launch the editor's agent with `--provider openai`, or choose OpenAI in its
provider footer. Model and reasoning controls follow the active provider.
`--provider openai --logout` signs out locally. `--provider openai --setup`
stores an explicitly supplied `OPENAI_API_KEY`; masked entry belongs in the TUI.
ACP preserves its existing checkpoint opt-in and host-specific UI exclusions.

## Storage and limits

Vesper owns its OS-keyring credentials, with an owner-only Unix file fallback
at the platform configuration directory's `agent-vesper/openai-credentials.json`.
`AGENT_VESPER_OPENAI_CREDENTIALS_PATH` is an advanced fallback-path override,
not a required setup step. A process-shared OS file lock serializes login,
refresh, mode changes, and logout across TUI and ACP. Tokens never enter normal
logs, session records, or configuration controls. Logout is local, not a claim
of server-side revocation.

Subscription transport is based on inspected upstream implementation, not a
published stable third-party subscription API. Endpoint policy or account
entitlement can change; authentication and access errors fail truthfully without
switching to paid API traffic. Foundation tests use synthetic loopback services,
not live accounts.

Subscription inference has no advertised server-side `max_output_tokens`
control. Vesper's auxiliary visible-output byte guard stops at an event boundary;
it cannot guarantee a bound on hidden reasoning or billed tokens. Audio,
OpenAI-hosted computer-use/image-generation tools, cloud tasks, and every Codex
application feature are not implied by this integration. Vesper's browser and
tool capabilities remain its own permission-gated implementations.

## Quick check

In a disposable workspace, ask: “Read README.md with read_file and summarize
it.” Confirm the tool event appears. Then ask it to create a small test file
and verify the permission prompt before approving. Repeat after explicitly
switching authentication modes if you have both account types. These are live
requests and may consume the selected account's allowance or API budget.
