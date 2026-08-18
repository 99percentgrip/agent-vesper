---
name: provider-routed-surface-decoupling
description: Make a harness surface (model/thinking/reasoning/auth) provider-routed so it works for every provider — current and future — with zero hardcoded provider match arms. Use whenever provider-specific behavior threatens to leak into the TUI/harness as a "zai"/"glm" match arm or a direct concrete-provider-crate import.
version: 1.1.0
author: Agent Vesper library (migrated from legacy GLM-ACP)
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [multi-provider, provider-routing, model-command, slash-command]
---

# Provider Routed Surface Decoupling

Multi-provider rule: the harness (TUI/dispatch/main) must NEVER name a
concrete provider. Every provider-specific behavior is
advertised/implemented by the owning provider and routed through a
provider-neutral port. "Superpowers" = each provider's own masterpiece; a
new provider registers and every harness function works with ZERO harness
edits.

PROCEDURE:

1. AUDIT first — grep the harness for coupling:
   `vesper_provider_glm::`, literal `"zai"`, provider-namespaced
   descriptor IDs (`"zai:model"`, `"zai:reasoning"`), and
   `provider_id == "zai"` / `!= "zai"` arms.
2. DEFINE a provider-neutral port (trait) in `vesper-provider` (e.g.
   `SuperpowerPolicy`, `ModelCatalog`, `ProviderCredentialPort`). Match by
   **alias** (`"model"`, `"thinking"`), NEVER by provider-namespaced
   descriptor ID. Add a permissive default impl for providers with no
   constraint.
3. IMPLEMENT the port behind the owning provider in
   `vesper-provider-glm`, porting the existing logic **verbatim**
   (behavior-preserving relocation, not a redesign).
4. EXPOSE via `ProviderRegistry` in `vesper-runtime`: add
   `register_with_X(...)` + a query returning `Arc<dyn ThePort>`
   (permissive default when unregistered/unknown).
5. REFACTOR the harness to route through the port: delete every hardcoded
   provider match arm + every direct concrete-crate import. The existing
   harness tests are the GLM-preservation gate — they must stay green
   byte-for-byte.

NEVER dismiss provider-coupling as "dead code to add later." That is the
recurring failure: it ships a contract violation (the project forbids
hardcoded provider match arms) and forces a bigger refactor later. Do it
provider-routed from the first commit. Verification: the permissive default
must let an unregistered/future provider work unchanged; the GLM impl must
keep Z.ai identical.

## Provenance

Learned in the legacy GLM-ACP agent on 2026-08-05; migrated to the Agent
Vesper library.
