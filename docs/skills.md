# Skills and automatic routing

[← Documentation](README.md)

Skills give Vesper procedures for particular kinds of work. Describe the task in
ordinary language; you do not have to memorize skill names. The complete library
stays available even when only a few skills are selected for a turn.

## Choose a routing mode

| Choice | Behavior |
|---|---|
| **Standard** | Default local metadata matching, without a separate model-selection call. |
| **Enhanced (preview)** | Local retrieval and task-contract checks over eligible skill metadata. |
| **Enhanced + Model-assisted selection** | Your configured provider chooses from a bounded shortlist before normal coding starts. Adds provider latency and usage. |

Model assistance is off by default. It aims to recognize the requested work and
distinguish related procedures, but can still miss a match or add an unnecessary
skill. Its selection does not grant tool permissions or make an unavailable
integration usable.

## Enable the preview in the terminal

1. Open **Settings → Skills** from the welcome screen, or use `/settings` while coding.
2. Turn **Model-assisted selection** on. This also selects **Enhanced** routing.
3. Leave Settings and choose **Save changes**.
4. Describe your next task normally, for example: “Turn these figures into an Excel workbook.”

Changes apply to the next turn and persist for this project. **Discard changes**
keeps the previous settings; **Keep editing** returns to the draft. You can also
enable or disable individual skills for this project without editing their files.

To return to the previous behavior, select **Standard** and save. To keep Enhanced
local retrieval while removing its extra provider call, turn **Model-assisted
selection** off and save.

## Editor and command controls

The ACP editor host and terminal share these controls:

```text
/skills settings status
/skills settings save model-assistance on
/skills settings save model-assistance off
/skills settings save mode standard
/skills settings save mode enhanced
/skills settings save disable <skill-name>
/skills settings save enable <skill-name>
```

These explicit `save` commands persist immediately for the project. The terminal's
ordinary Settings menus instead use the grouped save/discard prompt above.

For a specific procedure, use `/skill <name> [task]` or
`/skill bundle:<name> [task]`. An explicit request bypasses model-assisted relevance
selection, but still follows permission, availability and loading checks.

## What goes to the provider

When model assistance is enabled and there are eligible candidates, Vesper sends
the original task text and summaries of at most 12 candidates to the provider/model
already configured for coding. Each of the task and metadata payloads is bounded
to 8 KiB. Full skill bodies and expanded file/diff reference contents do not enter
this selection call. The normal coding request retains its usual context.

The selector has no executable tools, a 20-second deadline, and a requested output
limit of 1,024 tokens. It chooses up to three skills or abstains. Vesper checks the
result against current eligibility before loading the selected instructions.

This extra call consumes provider usage and adds latency; `/usage` shows available
account information. Usage that a provider does not report remains unavailable.
No separate account or embedding-model installation is required.

## If selection cannot complete

Routing notices identify selection or fallback. Provider errors, invalid responses
and oversized input use an observable local fallback. Stale decisions, unreadable
settings and index failures withhold model-selected activation. Cancellation stops
selection. An empty selection normally lets the ordinary coding turn continue.

If a task uses the wrong skill, describe the requested outcome more clearly,
select the skill explicitly, or disable that skill for the project. A missing
tool, platform or permission must be resolved through its normal setup or approval
path; selection itself cannot resolve it.

## Your library stays intact

Routing creates a temporary shortlist. It does not delete, archive, rewrite or
automatically disable skill files or references. Explicit per-project disable
settings also preserve the files. Project skills can override shared skills with
the same name; upgrades preserve existing edits and recorded seed deletions.

For measured results and remaining acceptance work, see the
[independent evaluation](foundation/skill-routing-model-live-evaluation.md).
