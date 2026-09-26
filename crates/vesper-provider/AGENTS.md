# Provider ports

## Purpose

Own provider-neutral request, capability, factory/session/catalog, stream,
continuation, fallback, and error contracts.

## Local Contracts

- `ProviderRequest::cache_routing_key` is a bounded, non-secret conversation
  identity. Providers may use it for cache affinity; they must ignore it when
  unsupported and must never derive it from prompt content, paths or secrets.
- `MediaCapability::maximum_bytes_per_item` is the provider-neutral preflight
  bound for materialized media. Hosts and adapters fail before dispatch when a
  known payload exceeds it; `None` means unknown, not unlimited.

- This crate implements no concrete provider and depends only on `vesper-domain`.
- SDK, HTTP, process-runtime, authentication, and core-loop types are prohibited.
- Capability fallback is typed and observable.
- Streams must have ordered events, exactly one terminal state, explicit
  visible-output tracking, and no plain replay after visible output.
- A provider may issue a bounded continuation from accumulated visible state
  only before any tool-call fragment appears. Complete calls recovered at a
  clean EOF keep their stable ID and are emitted once; ambiguous calls are
  terminal interruptions and are never replayed.
- Provider cancellation views are owned and remain usable for the lifetime of a
  returned stream.
- Explicit unsupported controls fail during request validation before dispatch.
- Provider-hosted tools use the shared descriptor/selection contract and remain
  distinct from client-side Vesper functions. They require explicit selection,
  declare egress/billing class, carry only bounded non-secret configuration,
  and unsupported adapters fail closed rather than dropping them.
- Credential ports optionally expose selected method identity, cancellable
  browser/device authorization, and local logout. Default implementations are
  inert; only explicit host actions may persist credentials. Adapters own
  callback validation and token persistence; challenge callbacks expose only
  user-facing authorization URLs/codes, never OAuth tokens.
- Authentication descriptors advertise their supported interactive login kinds.
  Hosts select browser/device flows from this metadata and must not infer them
  from a provider ID.
- Auxiliary request intent includes bounded structured memory extraction.
- `ProviderSession::query_usage` is an independent, read-only account query;
  its default explicitly reports unavailable account limits without inference.
  `ProviderUsage`, `UsageWindow`, and `render_usage` define the shared TUI/ACP
  panel. It uses aligned labels, compact context counts, solid allowance bars,
  and provider-reported reset clock times in the host's local timezone without
  an ASCII-art box. Elapsed reset times remain visible; the separate stale-data
  warning must not replace them. Other-day resets include a date and year.
  Context estimates
  stay distinct from account/billing limits; unknown values must never display
  as zero, full allowance, or unlimited.
- Capability requirements and same-provider candidates are bounded,
  provider-neutral ports. Payload scans fail closed on unknown capability and
  never infer support from model identifiers.
- Adapter-classified unsupported content may attach a typed requirement;
  consumers offer recovery only before visible output.
- `ProviderSession::auxiliary` optionally exposes the provider-neutral
  `AuxiliaryRequestPort` for bounded tool-free inference such as compaction.
  Sessions without one return `None`; callers retain a main-model or
  deterministic fallback and never assume auxiliary support.
- `ProviderSession::native_compaction` optionally exposes bounded opaque
  provider compaction. Advertising the port never activates it; the shared
  agent policy opts in explicitly, retains transaction ownership, and may
  fall back before commit. Core preserves the provider item without parsing.
- The [`ProviderSuperpowers`] trait and [`SuperpowerDescriptor`] advertise
  provider-native controls (effort dial, interleaved-thinking flag, model
  selector) so the composition boundary can render them without taking a
  dependency on a concrete adapter crate. The trait is **not** a supertrait of
  `ProviderFactory`; providers without superpowers simply omit the impl.
  Free-text controls use the shared 2,048-byte `Text` value; hosts use the
  shared parser/JSON projection and adapters retain semantic validation.

## Verification

- Run `cargo test -p vesper-provider`.
- Run `cargo xtask architecture`.

## Child DOX Index

No children.
