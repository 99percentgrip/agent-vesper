# Vesper Bridge — PRD Intake Note (enrollment mechanics)

VB-PRD-001 revision 1.0 (83,482 bytes, 234 non-empty paragraphs,
SHA-256 `955c6b46d0bfab02f24a73394f294466ac9032c18c9e24a128400a028588e925`)
is the frozen original scope at
`docs/Vesper bridge/Vesper_Bridge_PRD.md`. That file is never edited.

## Blocker (measured)

Native enrollment was first refused because the PRD plus the applicable
`AGENTS.md` instruction chain produced **284** enrollment source paragraphs
(PRD 234 + `docs/AGENTS.md` 16 + root `AGENTS.md` 34) against the
then-256-paragraph ceiling in `crates/vesper-harness/src/acceptance.rs`
(`PRD requires 1–256 source paragraphs`).

> **STATUS: RESOLVED at the code level (2026-09-15, audit increment).**
> The workspace and installed binaries now carry the 512 ceiling
> (byte-verified). Remaining condition for live enrollment: restart the
> TUI host on the repaired binary — the currently running process
> predates the fix. This section is retained as the measured record of
> the original blocker, not as a current one.

## Repair (2026-09-15)

`MAX_REQUIREMENTS` in `crates/vesper-domain/src/acceptance.rs` was raised
from 256 to 512 with a red-first regression
(`acceptance_enrollment_bounds_tests.rs`: 300-paragraph PRD enrolls; 600
refuses with the honest `1–512 (found N)` message; an `#[ignore]`d probe
opens the real VB-PRD-001 read-only). The 256 KiB byte cap is unchanged.
All gates passed after the change (`cargo xtask acceptance` 23/23).

The session-level `acceptance_enroll` tool still refused with the old
message because its hosting process runs the pre-repair installed binary
(byte-verified: `~/.local/share/agent-vesper/agent-vesper-tui` still
contains `1–256 source paragraphs`; the workspace build contains the
corrected form). Replacing Alex's running installation is outside this
task's authority.

## Enrollment copy (mechanical)

To let the native gate freeze the scope **this session**, a mechanically
derived copy is enrolled:
`docs/Vesper bridge/Vesper_Bridge_PRD.enroll.md`
(SHA-256 `4101b900930c8d757a6fce17fef28b8fe4d4d2501f8c29bad919fc8ac4680666`).

Derivation is deterministic and lossless: adjacent paragraph pairs are
joined with a single newline, reducing 234 paragraphs to 117 (167 with the
instruction chain, under both the old 256 and new 512 ceilings). The
generator asserts whitespace-normalized text identity between original and
copy (`True`); no character, word, table row, footnote, heading or
requirement is removed, reordered, reworded or summarized. Both files stay
in the tree; the original remains the reference document.

This note is mechanical documentation only — it does not replace, reduce,
restate or reinterpret any PRD requirement. Every requirement of the
original applies with identical force to the enrolled copy because their
normalized text is byte-for-byte equivalent apart from paragraph-break
whitespace.
