# ADR 0028: Native implementation acceptance

Status: accepted. Date: 2026-09-11.

## Decision

Implement the approved [completion-assurance design](../foundation/completion-assurance-proposal.md)
inside the Rust foundation and both native hosts. Completion of an enrolled PRD
objective is a harness decision, separate from provider stop and plan checkmarks.
The user explicitly approved implementation and verification.

- `vesper-domain::acceptance` owns strict version-1 contract, platform, evidence,
  finding, receipt and report DTOs. These are data, not an importable authority.
- `vesper-agent::acceptance` owns deterministic coverage/evidence evaluation and
  the injected `CompletionPort`. `AgentTurnOutcome::Acceptance` carries the
  authoritative verdict. A successful ordinary turn is not a PRD certificate.
- `vesper-harness::acceptance` owns live authority outside provider context:
  bounded original paragraphs, frozen contract, independently reviewed check
  definitions, collector-owned receipts, findings and scope lineage.
- Native `/acceptance start <PRD>` enrolls one objective. Settings → Implementation
  acceptance in the TUI and `/settings acceptance on <PRD>` in ACP save activation
  for subsequent prompts in that workspace. Default reads create no state.
  Stop/off/revise are user routes, never model tools. Scope changes do not complete
  the original objective. Invalid saved settings refuse dispatch.
- A fresh provider-neutral reviewer reads original paragraphs and applicable
  project rules. Coverage proposals receive up to three bounded correction
  attempts. Check-adequacy and final review require observed successful source
  reads in a read-only tool registry. All source/scenario IDs must be accounted
  for. Review is an additional challenge, not a correctness oracle.
- The first check format is a typed exact Cargo library/integration test selector,
  with explicit evidence class, platform, features and deadline. Check admission
  must match the frozen scenarios. Only the collector creates live receipts;
  no upload, approval boolean or requirement-removal tool exists.
- Verification captures bounded source bytes, including untracked files, build
  definitions and known runtime settings. It materializes private temporary
  copies; a live-tree edit cannot change those retained inputs. Checks bind source,
  contract, check definitions, selected Rust toolchain, environment and argv;
  receipts also identify their native collector and observation time.
  Nonzero exit, zero matches, skips, malformed/oversized output, cancellation,
  timeout and cleanup uncertainty cannot pass. Current source is checked again
  before publication. Receipts describe their tested snapshot, not future edits.
- AgentLoop holds model content/reasoning deltas during gated turns, preserves
  tool transactions and bounded draft metadata, and renders the terminal report
  itself. Missing evidence feeds bounded autonomous repair. Four consecutive
  unchanged failed stops or the ultimate iteration ceiling remain incomplete.
- VRO candidates, ReAct Finish and Swarm reports are delegated drafts. Native
  parent composition freezes obligations before delegated work and returns through
  the shared repair/publication boundary afterward. Child success cannot certify
  a parent. Unverified candidates do not feed success learning. ACP's documented
  ReAct strategy continues through its native AgentLoop path.
- Export is explicit, bounded audit data. Resume reconstructs the original scope
  and retains lineage, but discards imported verification authority. Compaction
  cannot erase live obligations. Restart requires fresh review and checks.
- `cargo xtask acceptance` runs named cases with positive execution counts;
  deleted/renamed/ignored tests fail the gate. `acceptance-mutations` verifies that
  two intentional evaluator defects are caught, in a temporary source copy.
  Canonical, MSRV and platform CI run the exact acceptance gate. Public releases
  remain subject to the existing exact-commit workflow prerequisites.

## Boundaries and consequences

Activation is scoped and visible. This does not intercept an external coding
agent's chat or automatically infer a complete specification from arbitrary
conversation. External agents can run the repository's Rust regression gate;
that certifies these regression cases, not every PRD they may implement.

Semantic coverage and test adequacy still involve fallible model review. Tests
must challenge production behavior; neither hashes nor reviewer agreement prove
that undiscovered defects are absent. Hostile code with the same OS identity is
outside the cryptographic trust boundary. Private snapshot copies and read-only
source permissions are defense against accidental/concurrent edits, not a new
sandbox or proof against malicious self-modifying test binaries. Existing sandbox
and permission demands are retained; no weaker execution fallback is allowed.

Snapshots refuse symlinks, special files, oversized inputs and private `.env`
files. Generated/private roots are excluded and disclosed to review. Requirements
that depend on omitted inputs, unavailable platforms, missing toolchains or native
dependencies remain incomplete. Initial receipts execute Rust tests; other
verification protocols need their own trusted collectors before they can certify
requirements. Multi-platform evidence is evaluated on its executing platform;
remote audit JSON is not silently promoted into a trusted local receipt.

## Verification

See [implementation evidence](../foundation/completion-assurance-execution.md)
for executed commands, measured cases, limitations and current release status.
No third-party implementation was copied and no production dependency on the
researched workflows was added. MSRV remains Rust 1.88 and production is safe Rust.
