# VRO-18.1 — Provider Authentication Inside Settings

Product: Agent Vesper
Type: Bounded corrective implementation; extension of VRO-18, not a new provider project
Status: Implemented in the working tree; user acceptance and release pending
Repository: 99percentgrip/agent-vesper
Inspected baseline: `52d36202226c1932be9d8304a32e88c4a40cd3ce` (origin/main after v0.24.1)

## Problem

API-key authentication was reachable through `/auth`, but Settings → Providers
only offered provider choice, Save, and Cancel. Authentication has to be
manageable inside Settings for every registered provider without typing
`/auth`, editing a config file, exporting an environment variable, or
restarting just to open another provider's panel. `/auth` remains a shortcut
to the same panel.

## Required behavior

- Settings → Providers highlights a provider. Enter/Space still chooses the
  draft provider. **Manage authentication**, or **M** on the highlighted row,
  opens Authentication and does not activate that provider.
- Methods, labels, browser/device actions, and key URLs come from
  `ProviderDescriptor`. A synthetic unfamiliar provider id must work without a
  new provider-name branch.
- Multi-method providers (xAI, OpenAI) show every method even when one is
  already selected. Z.ai shows only its API key. LM Studio shows its optional
  key and may say no authentication is required only because the adapter marks
  the method optional.
- Key entry is masked. Control characters are rejected, not stripped into a
  different secret. Saves, method selection, removal, and sign-out are
  immediate and are not rolled back by Settings Discard.
- Cancel or a failed login keeps the previous credential. Storing or selecting
  one method must not delete the other method's stored secret. Sign-out removes
  every Vesper-stored credential for that provider only and must not claim an
  environment variable was removed.
- The next xAI turn uses the committed method. A Grok-session selection drops
  stale API-only hosted tools even when the startup surface still advertises
  them. Explicit API selection does not fall through to subscription billing.
- Browser and device sign-in keep the provider-owned flows. The complete
  authorization URL is copied or opened as a single argument, never rebuilt
  from wrapped terminal text.

## Ledger

| ID | Outcome |
| --- | --- |
| AS-01 | Settings navigation reaches authentication for every registered provider. |
| AS-02 | Advertised methods stay visible; current selection is marked. |
| AS-03 | Managing an inactive provider does not change the active provider. |
| AS-04 | Key entry, replacement, and removal are masked and scoped. |
| AS-05 | Browser/device actions reuse native flows and the complete URL. |
| AS-06 | Cancel or failure preserves the prior credential. |
| AS-07 | Sign-out and environment-managed state are truthful. |
| AS-08 | Auth is immediate and distinct from Settings Save/Discard. |
| AS-09 | The next request uses the selected method. |
| AS-10 | Hosted-tool projection follows the auth transition. |
| AS-11 | Late or repeated input cannot apply a result to the wrong provider. |
| AS-12 | Settings, `/auth`, startup, and ACP use provider credential ports. |
| AS-13 | An unfamiliar provider id needs no UI name branch. |
| AS-14 | Secrets stay out of frames, logs, and Settings JSON. |
| AS-15 | Real Settings navigation tests prove the route. User acceptance is separate. |

Audit 2 and Audit 3 are not part of this correction.
