# Architecture Reconnaissance

## Purpose

Own read-only external-repository pattern reconnaissance synthesized into
Vesper terminology, feeding future feature PRDs (e.g. VRO-16 hive governance).

## Ownership

- `recon_existing_voice_stack.md` owns the read-only inventory of
  Vesper's own shipped voice capabilities (push-to-talk trace, the
  speech-output negative result, reuse-vs-gap matrix, and the
  recommendation-only PR-2 scope correction) that gates PR-2 planning.
- Reports in this directory analyze external repositories from documentation
  and configuration only; production source files are never ingested.
- Each report pins the analyzed external commits, records licenses, and maps
  discovered mechanisms onto existing Vesper primitives — never onto invented
  ones.
- Feature requirement documents remain owned by root PRD files and ADRs; this
  directory owns the reconnaissance evidence that informs them.

## Local Contracts

- Read-only with respect to both the Vesper workspace and external sources:
  no production code changes result from a reconnaissance mission.
- Temporary external clones live under ignored scratch space and are removed
  at mission closeout; no external repository state persists in the tree.
- Every mechanism-to-primitive mapping must cite current workspace evidence;
  aspirational mappings must be marked as gaps or proposals.
- External license obligations are recorded per report; AGPL-licensed sources
  are studied for concepts only.
- External upstreams are referenced exclusively by their assigned aliases
  (e.g. `governance alpha`, `governance beta` — never upstream brand names,
  URLs, or branded file/command identifiers). New reconnaissance reports
  request aliases from Alex before first use; aliases follow the
  `web oracle alpha/beta/gamma` precedent and are never used bare.
- Authorized deviations are recorded in place: the voice-oracle mission
  (recon_voice_oracle.md) was directed to analyze the cloned implementation
  itself, so that mission read upstream production source (MIT, pinned
  commit) rather than docs/config only. The deviation is noted in the
  report and in `docs/foundation/voice-oracle-recon-execution.md`; it does
  not generalize to other missions.
- VRO-18 explicitly requires the first-party xAI and Grok Build names, URLs,
  documentation, and pinned production-source symbols in its evidence. That
  user-authorized source audit is limited to
  `recon_xai_native_provider.md` and its linked PR-0 execution report; it does
  not relax the alias or source-ingestion rule for other reconnaissance.

## Work Guidance

- Prefer bounded doc/config slices (README, design docs, YAML) over
  whole-file reads to bound context ingestion.

## Verification

- Confirm every workspace path cited in a report exists.
- Confirm external-commit pins and licenses are recorded.
- Confirm scratch clone directories are absent at closeout.

## Child DOX Index

No children.
