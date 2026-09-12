# Native output visual upgrade execution

Date: 2026-09-13. Status: implemented and Linux-verified; native macOS/Windows
visual acceptance remains unexecuted. No release or installation during implementation verification. Subsequent
publication is recorded in [v0.22.3 release execution](v0.22.3-release-execution.md).
Owning scope: [output visual upgrade PRD](../output-visual-upgrade-prd.md).

## Objective

Replace the bland activity output and misaligned reports in Alex's screenshots
with colorful chronological activity, truthful animated status dots, readable
numbered diffs, and aligned responsive reports.

## Methods and implementation

- Inspected the real TUI projection, Markdown parser, frame/scroll/click mapping,
  direct and ReAct progress adapters, shared command executors and ACP progress port.
- Added theme-aware lexical colors, cell/grapheme wrapping and chronological activity
  projection. Commands retain bounded excerpts and Ctrl+T expands their details.
- Applied report wrapping before role padding, retained hanging list indents and
  rendered pipe tables as aligned columns or stacked labeled values. Malformed
  extra-cell rows remain literal rather than losing data.
- Added optional source starting-line metadata to mutation previews, preserving
  legacy deserialization. Old/new counters advance according to actual diff kind.
- Added shared bounded command excerpts with ANSI removal and known-pattern
  credential scrubbing. Non-command results remain summaries. Both hosts receive
  the same outcome/excerpt; ACP editors continue to own their visual presentation.
- Found and corrected a functional prerequisite: the unsandboxed executor ignored
  nonzero exit status, and both executor routes treated timeouts as successful
  results. They now emit failures, retaining bounded diagnostics. The sandbox
  backend already rejected nonzero exits; that behavior was preserved.

## Changed files

- `apps/agent-vesper-tui/src/{activity,presentation,markdown,ui}.rs`: activity,
  syntax, wrapping, reports, diffs, scroll/click projection, reference-frame tests.
- `apps/agent-vesper-tui/src/{lib,main}.rs`: module wiring and direct/ReAct mapping.
- `crates/vesper-domain/src/tool.rs`: optional preview starting line.
- `crates/vesper-agent/src/{agent_loop,tools}.rs`: bounded excerpts, real shell
  outcomes and mutation source coordinates.
- `crates/vesper-agent/tests/{agent_loop,executors}.rs`: actual command success,
  nonzero exit, timeout and resulting progress evidence.
- `apps/agent-vesper-acp/src/lib.rs`: shared excerpt forwarding and mapping test.
- Owning root/application/crate/docs AGENTS, this report, PRD, evidence index and
  reference images: operational contracts and verification records.

## Evidence

Base commit: `283b7c195493d9a798100a313fcdcbf81ec7317f`; implementation was verified before commit.
Release commits and publication receipts are recorded in the linked release report.
SHA-256 of the 12 changed/new Rust files, sorted by repository path and encoded
as `path + NUL + bytes + NUL`:
`caf44deaf40de3d7bc8b4baad7ebbfeb94391f268643c33ff169247689e0e2a1`.

| Check | Exact command / evidence | Result |
| --- | --- | --- |
| Repository gates | `cargo xtask verify` | Exit 0; workspace 2,247 passed, 0 failed, 34 ignored; fmt, all-target/all-feature clippy, architecture, contracts, fixtures, naming and remaining xtask gates passed. ADR 0028: all 23 exact acceptance cases passed. |
| Final renderer | `VESPER_OUTPUT_CAPTURE_DIR=/tmp/vesper-output-captures cargo test -p agent-vesper-tui --all-features --lib --offline` | 240 passed, 0 failed. Includes the final malformed-table regression added after the workspace run. |
| MSRV | `cargo +1.88.0 test -p agent-vesper-tui -p agent-vesper-acp -p vesper-agent -p vesper-domain --all-features --lib --bins --locked --offline` | 924 passed, 0 failed, 3 ignored. |
| Final TUI lint/build | `cargo clippy -p agent-vesper-tui --all-targets --all-features --offline -- -D warnings`; `cargo build -p agent-vesper-tui --all-features --offline` | Both exit 0. |
| Native terminal regression | `python3 apps/agent-vesper-tui/tests/settings_pty.py target/debug/agent-vesper-tui` | PASS: real mouse navigation, discard, keep editing, grouped save, acceptance toggle, active permission and restart persistence; isolated HOME/workspace, no provider prompt. This is a Settings regression, not a live-provider output screenshot. |
| Whitespace | `git diff --check` | Passed. |

The workspace run covers production changes; the final renderer run and MSRV run
also cover the last extra-cell table test and reference-fixture adjustment.
Ignored tests remain explicit platform/container gates; no ignored case is counted
as a pass. No new production dependency was introduced.

Requirement trace:

- **V1–V3:** `activity::tests::{chronological_results_stay_paired_and_pending_does_not_become_success,react_duplicate_start_folding_and_links_preserve_one_call}`;
  `presentation::tests` color/blink and syntax checks;
  `agent_loop::output_preview_tests::shell_excerpts_scrub_credentials_strip_ansi_and_bound_output`;
  `real_shell_exit_status_and_excerpt_reach_progress_events` executes real shell
  success/nonzero exit through the shared loop. Existing executor timeout regression
  now requires an error. TUI direct and ReAct mapping tests passed.
- **V4:** `preview_source_numbers_start_at_real_context_and_legacy_remains_unknown`
  edits line 90 of a 100-line source and asserts preview start 88; native frame
  tests assert actual 217–221 gutters and existing diff-background/count tests pass.
- **V5–V6:** `ui::output_upgrade_reference` checks full frames for six themes ×
  three widths and both animation phases; wrapped reports retain role/list indents.
  Markdown report tests cover 20/40/80/120 columns, code/escaped pipes and malformed
  extra cells. Existing Unicode, partial Markdown, scrolling, URL mapping and
  degenerate resize tests passed. Screen-reader activity uses plain labels/state.
- **V7:** `tool_started_and_finished_pair_by_outstanding_id` also asserts ACP
  receives the failure excerpt instead of its size summary; TUI progress tests
  assert the same excerpt reaches activity. The shared real-shell regression proves
  the emitted outcome comes from the executor, not inferred output text.

Logs remain in `/tmp/vesper-output-{verify,render-final,msrv,clippy-final,build,pty}.log`.
SHA-256 receipts: verify
`036fea2db3f064c16fe433700982456c2ac86561fcf9b444b8091c38d1105dd2`;
final renderer `c2040710fa9ed232b1069c30bf8ad4e67a86587515c291bdda558795f89b7a53`;
MSRV `77312af1ab19d5e8465e569d542a4f53993a2cca077ed58510a00e5a9ce5d2e7`.

### Actual renderer reference captures

[Dark, 120 columns](output-reference-dark.png), [Nord, 80 columns](output-reference-nord.png),
and [Light, 80 columns](output-reference-light.png) are rasterizations of the actual
`render_to_frame` TestBackend cells with a synthetic fixture. They are not live
provider receipts or generated design mockups. Noto font fallback is used; the empty
lower viewport/composer is cropped. Emoji may differ from the user's terminal.

Reproduce by creating `/tmp/vesper-output-captures`, running the final renderer
command above, then `python3 docs/foundation/output-reference-render.py` (Pillow and
Linux Noto fonts). The fixture captures 18 theme/width buffers; status assertions
compare two clock phases and ensure completed dots remain stable. The PNGs are
static and do not themselves demonstrate blinking.

Image SHA-256: dark `1ad7d4ad46ce2d078c198aa6cb6453e9c03bf81120453560e0461398b024b475`;
Nord `f8c508f03bba3f3cfbec6134be2aece45d505a04f99b697095de2c769ffca368`;
light `a51a0c06b8fae349f49cb7ce45e2c1997860d8405a595a352de296cc32c93c18`.

## Deviations and failures encountered

- Old rendering assertions expected raw headings, uniform telemetry colors and
  raw ReAct read-file output. Updated them to the requested rendering contracts;
  full file bytes still return to the agent, while activity shows size summaries.
- Early checks exposed a swallowed review URL during activity projection, an
  overstrict wrapping assertion and a malformed-table extra-cell loss. Corrected
  those cases and retained regressions. Lint checks caught test-only helper
  dead code and two test formatting/type issues; corrected before final checks.
- The initial sandboxed socket test was refused by the execution environment.
  Socket/process verification runs with approved escalation and isolated fixtures.

## Unresolved items and readiness effect

- Native Windows/macOS terminal visual execution and Alex's subjective comparison
  have not been performed. Font/emoji rendering is terminal-dependent; no claim of
  universal pixel-for-pixel screenshot identity is made.
- Lexical coloring is not a full language parser. Unknown languages remain literal.
- Excerpts are bounded and known-pattern-scrubbed, not a universal secret filter.
- No live provider calls, public release, version bump, or local installation are
  part of this work. The installed 0.22.2 remains for the user-reserved updater test.

## DOX closeout

Updated the nearest TUI, ACP, agent, domain and documentation contracts and root
preferences. `apps/AGENTS.md` and `crates/AGENTS.md` retain the existing composition
and dependency rules; their child indexes are unchanged. The TUI tests child stays
unchanged because new Rust rendering tests live beside source and its existing
ownership already states that rule. No new child DOX boundary was introduced.
Pre-existing 0.22.2 installation-report edits were preserved as a separate record.
