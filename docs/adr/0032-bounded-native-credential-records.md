# ADR 0032: Bounded native credential records

Status: ACCEPTED

Refines: [ADR 0014](0014-agent-vesper-authentication.md).

## Context

Windows Credential Manager limits a generic credential blob to 2560 bytes.
The pinned Windows keyring backend encodes passwords as UTF-16 before applying
that bound. An OpenAI subscription record contains multiple tokens and can
exceed the bound even when each token passes its adapter validation. Unix
fallback behavior concealed this platform difference. Successful navigation to
an authentication menu does not prove token persistence.

Primary contract: [Microsoft CREDENTIALW](https://learn.microsoft.com/en-us/windows/win32/api/wincred/ns-wincred-credentialw).

## Decision

- Keep provider-neutral persistence in `vesper-auth`; neither host nor adapter
  gets a Windows-specific provider shortcut.
- Bound a composite credential to 256 KiB and a serialized Unix vault to 1 MiB.
  Adapter-specific token/key validation remains independent. Reject an oversized
  vault before replacing its previous readable value.
- Preserve small Windows native entries. Larger records use immutable UUID
  generations of UTF-8 chunks of at most 1200 bytes (at most 2400 UTF-16 bytes),
  with a bounded primary descriptor carrying count, byte length and SHA-256.
- Persist a bounded native transaction journal before generation writes. Verify
  all chunks before atomically replacing and reading back the primary entry.
  Interrupted writes preserve the previous credential; pending cleanup remains
  owned by the journal and is settled before another mutation.
- Sign-out journals its deletion intent first. Reads never expose a credential
  with a pending deletion intent. Confirm absent primary readback and remove
  tracked generations before deleting the journal. Ambiguous independent primary
  changes are refused, never guessed or overwritten.
- Serialize Windows operations with a bounded cross-process file lease beside
  the explicitly configured vault. This file contains no secret. All processes
  accessing the same native identity must use the same configured vault path.
- Retain existing read-only vault precedence, including explicit fixture vaults.
  Windows refuses mutations before native writes/deletes if a retained vault
  credential cannot be permission-verifiably retired. This prevents stale-file
  resurrection or claiming an effective native replacement while an old file wins.
- Windows never writes a plaintext fallback. Read failures remain failures,
  rather than being presented as absent credentials.

## Compatibility

Existing small native API keys remain readable. Linux/macOS native-first and
owner-only Unix fallback behavior remains available. Storage receipts describe
which backend actually accepted a write. Both hosts continue using the same
adapter credential ports.

## Security consequences

Chunks and journals stay in the user's native credential manager. Metadata and
errors never contain credential values. Descriptors, generations, counts and
assembled lengths are bounded; missing or altered chunks fail closed.

ADR 0014's verification restriction is refined for one explicitly invoked
hosted-platform acceptance lane: synthetic credentials may be saved in UUID
namespaced entries owned solely by the fixture on disposable GitHub runners.
The fixture checks a fresh-process reload and removes its entries. It requires
both hosted-runner identity and explicit opt-in, never reads real user entries,
and never contacts a provider. Ordinary foundation verification continues to
use private deterministic backends; native acceptance is ignored by default.

## Migration consequences

No startup migration writes user credentials. The next explicit save creates
new bounded native entries; rotation and sign-out settle their tracked cleanup.
Accepted ADR 0014 is retained unchanged as the historical decision.

## Verification

- `cargo test -p vesper-auth --all-features` exercises the Windows-sized fake
  backend, interrupted writes, rotation, legacy keys, Unicode, maximum sizes,
  integrity failures, provider isolation and sign-out recovery.
- Explicit five-target `native_persistence` hosted acceptance uses the public
  production store, multiple synthetic subscription tokens, a fresh child
  process, rotation and removal. Windows must return `NativeKeyring` and must
  not create a plaintext vault; Unix reports its actual backend. The CI collector
  and child reload both require one named passing exact receipt; zero-match,
  ignored, renamed and failed cases cannot satisfy native proof.
- Strict Clippy, Windows target compilation, architecture, workspace tests and
  the existing OpenAI device-auth fixtures remain required.
- Current execution evidence: [Windows credential persistence repair](../foundation/2026-10-08-windows-credential-persistence-repair.md).
