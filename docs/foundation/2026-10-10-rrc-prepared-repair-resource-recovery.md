# RRC prepared repair resource recovery — 2026-10-10

## Objective and current boundary

Continue the approved Windows/macOS voice repair and single v0.24.12 release
through provider-neutral native RRC. No user installation is authorized.
Source corrections are implemented; current native release/device evidence
remains pending. No complete parity or unlimited-dictation claim is made.

## Observations and methods

- Read all four completed workflows for candidate
  `9db1d89758bbbc590f6b10c4f1c78aee0012e435`: twelve terminal jobs,
  nine successful, three failed. Canonical Windows installer and MSRV passed.
  Both Linux lanes executed production recognition twice, dependency setup and
  both Natural Voice signals. Windows/Mac downstream speech checks did not run.
- Mac failures were the same stale `<120 MiB` retained-pack test band. Pinned
  manifests retain 133,047,356 bytes on ARM Mac and 133,479,640 bytes on Intel
  Mac, within unchanged 256 MiB retained / 512 MiB peak production budgets.
- The Windows test made only `bin/`, then wrote native `Scripts/python.exe`.
  Creating the resolved interpreter parent fixes fixture construction without
  changing production precedence.
- Resumed foreground RRC passed eight local gates, read the complete matrix and
  autonomously prepared both repairs in one isolated turn. Its accepted package
  proof passed. Critical swap growth stopped canonical verification. RRC wrongly
  marked that interruption as a disproven hypothesis and dispatched another
  model turn. Cancelled safely before promotion; retained the complete
  [checkpoint](2026-10-10-rrc-prepared-repair-resource-failure.json), failures,
  original repair and admission counts. No journal clearing or budget reset.
- Earlier disk deferral resumed automatically after bounded historical
  incremental-cache reclamation with no active compiler. Receipt:
  `/tmp/vesper-rrc-stale-incremental-cleanup-20261010.json`. No source, journal,
  installation or application data was removed.

## Changes and files

- `release_executor.rs`: retry verification of the same frozen repair after
  three safe resource observations, with fresh native snapshot checks. Do not
  dispatch another model, disprove the hypothesis or charge retry budget for
  pressure. Cancellation, permission, watchdog and uncertain-mutation errors
  propagate as controller stops.
- `release_recovery.rs`: optional persisted recovery destination preserves the
  diagnosis stage; legacy records default to local verification. Invalid
  destinations refuse. Both hosts share the truthful paused-repair projection.
- Include the native worker's Windows fixture and Mac budget corrections in
  this candidate; its [report](2026-10-10-native-voice-foundation-ci-repair.md)
  now records the actual package command and canonical interruption.
- `xtask`: three new cases are mandatory on every native target: prepared-repair
  recovery, controller-stop refusal and two-provider native canonical-stop
  composition.

## Exact verification evidence

Local checks use one Cargo job/test thread, temporary Git roots and fixture
providers; no live provider, credentials, installation, microphone or speaker.

- `cargo test -p vesper-harness --lib --all-features prepared_repair_`:
  initial two regressions passed. Persisted recovery requires three safe
  observations and preserves source evidence, proof and admissions; unsafe
  snapshots/destinations and cancellation refuse.
- Qualified exact `isolated_repair_controller_stops_do_not_record_failed_hypotheses`:
  passed through real tools/commands, both fixture provider IDs and remote and
  preparation routes. Stops cannot promote source or record failed hypotheses.
- Removing the pressure-recovery branch: **0 passed, 2 failed**, expected
  assertions. Removing the controller-stop guard: **0 passed, 1 failed**,
  expected native composition assertion. Final source restored afterward.
- Restored final resource regressions: **2 passed, 0 failed**. Restored exact
  native controller-stop composition: **1 passed, 0 failed**. Qualified Windows
  interpreter fixture and Kokoro pack budget: **1 passed, 0 failed each**.
- Strict Clippy, acceptance, MSRV and exact-source native gates: pending
  receipts. No older-source proof certifies the new code.

Logs: `/tmp/vesper-rrc-prepared-repair-resource-{red,green}.log` and
`/tmp/vesper-rrc-controller-stop-exact-{red,green}.log`. An earlier unqualified
`--exact` selection matched zero tests and is not proof; the qualified run
executed one test. Fixture providers do not establish live model effectiveness.

Durable local logs: [receipt manifest](2026-10-10-rrc-prepared-repair-resource-recovery-receipts.json),
[receipt archive](2026-10-10-rrc-prepared-repair-resource-recovery-receipts.tar.gz).

## DOX pass

Updated harness/xtask contracts, documentation ownership, evidence index and
owning PRD links. Root/apps/crates boundaries and child indices are unchanged.
TUI/Kokoro owning docs are unchanged intentionally: the fixture and descriptive
test band do not alter production path/audio/setup contracts.

## Deviations and unresolved items

- Cancellation preserves completed work and does not imply rollback or remote
  cancellation. Ordinary admission archived the user-cancelled epoch and
  revalidated locally; that is not direct checkpoint-resume proof.
- Active resource recovery retains proof in its owning worker. Owner loss
  during unfinished repair remains fenced by the existing mutation journal;
  a resume destination alone cannot replay or settle uncertain work.
- Fresh Windows/Mac recognition/synthesis lanes, real microphone/speaker/OS
  permissions and original Windows laptop acceptance remain distinct requirements.
- Existing Linux-equivalent 120-second / 4 MiB clip limits remain; unlimited
  dictation is not implemented or certified.
- No v0.24.12 tag, publication, Registry update or installation is claimed
  until actual native receipts establish it. No second version is planned.

## Readiness effect

Addresses the observed resource-error classification defect and two proven
foundation blockers. Fresh native gates and actual controller closeout remain
necessary before release readiness.
