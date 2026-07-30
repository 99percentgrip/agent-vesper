# Security Policy

Agent Vesper is under active migration and is not yet a production agent harness.
Do not use this Stage 1 workspace to execute untrusted agent actions or handle
provider credentials.

## Reporting

Report security issues privately to the repository owner rather than opening a
public issue containing exploit details, credentials, private session data, or
secret canaries. No separate public security contact is asserted by this
repository.

## Foundation invariants

- Policy denial is absolute; Bypass cannot override it.
- Providers and frontends never own policy or execution authority.
- Secrets have redacted `Debug`/`Display`, cannot serialize, and require explicit
  exposure.
- Agent Vesper uses independent state roots and never silently modifies legacy
  Native GLM ACP state.
- Required isolation fails closed when unavailable.
- Reasoning is excluded from generic logs, telemetry, indexes, and other sinks.
- The Python process-descendant cancellation leak is a negative fixture that Rust
  must fix rather than reproduce.

See [security invariants](docs/recon/security-invariants.md) and accepted
[ADRs](docs/adr/). Dependencies and advisories are governed by `deny.toml` and CI.
