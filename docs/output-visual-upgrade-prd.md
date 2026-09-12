# Native output and report visual upgrade

Owner: native TUI presentation; shared tool execution owns factual outcomes.
Directive: Alex's 2026-09-13 old/new output and report screenshots.
Status and exact evidence: [execution report](foundation/output-visual-upgrade-execution.md).

## Required behavior

| ID | Requirement | Acceptance |
| --- | --- | --- |
| V1 | Read, search/exploration, edit and command activity uses clear blue labels, distinct command/source token colors and nested output, in execution order. | Real renderer frames; direct and ReAct host tests. |
| V2 | Successful operations show green dots; running operations show blinking orange dots; failures show red dots. Missing completion never becomes success. | Two animation phases, interrupted-call tests and real shell exit/progress regression. |
| V3 | Command output is readable and bounded. Compact rows expand with Ctrl+T; empty output and truncation are explicit. | Fold/unfold, no-output and excerpt-bound tests; both host mappings. |
| V4 | Diffs retain addition/deletion backgrounds and signed counts, add syntax colors and truthful old/new line numbers. | Mutation-produced starting-line regression, legacy decode and frame cells. |
| V5 | Reports have one consistent left edge, hanging list indents and readable emphasis, code and links. Tables align at wide widths and preserve labeled values when narrow. | Report wrapping and malformed-table tests; frames at 40/80/120 columns. |
| V6 | Selected themes, scrolling, URL hit-testing and accessibility remain functional; source text, Unicode and streaming partial Markdown remain readable. | Six-theme frame matrix; existing streaming, scrolling, hyperlink and tiny-size tests; accessibility projection. |
| V7 | Shared output/outcome behavior reaches ACP and TUI; terminal rendering remains host-owned. | AgentLoop real-shell test, ACP event mapping and TUI direct/ReAct mapping tests. |

## Constraints and delivery

- Match the reference's hierarchy, colors, status behavior and alignment. Do not
  claim pixel identity across terminal fonts, emoji renderers or platform palettes.
- Syntax highlighting is lexical for supported source languages; unknown languages
  retain literal source. It must never alter or execute the displayed code.
- Output excerpts strip terminal controls and known credential patterns. This is
  bounded presentation, not a guarantee that arbitrary shell output is nonsensitive.
- No provider calls or real user-state writes in verification. Public release,
  version bump and installation are separate from this implementation. Alex owns
  local update acceptance; release work does not authorize local installation.
- The execution report records current evidence, failures encountered and remaining
  platform/user acceptance. Code and plan checkmarks alone do not close requirements.

[Release execution](foundation/v0.22.3-release-execution.md) tracks the authorized
0.22.3 publication, the installation preserved before testing, and Alex’s
subsequent successful Linux update and restart.

[Front-page refresh](foundation/readme-refresh-execution.md) records user-facing
publication of the shipped controls and the subsequent user-supplied Linux updater
acceptance in the v0.22.3 release report.
