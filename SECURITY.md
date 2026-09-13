# Security Policy

Agent Vesper is a Rust coding agent with a native terminal interface and an ACP
editor server. It can read and modify files, run commands, and connect to the
model provider you configure. Access is governed by session permissions and
policy; isolation depends on the configured backend and available capabilities.

## Reporting a vulnerability

Report suspected vulnerabilities privately to the repository owner. Do not put
exploit details, credentials, or private session data in public issues.

Include the affected Vesper version, operating system, relevant configuration,
expected behavior, and minimal reproduction steps. Redact secrets and private
project content. This repository does not currently publish a separate security
email address or response-time commitment.

## Permissions and data

- Review the selected permissions before allowing file changes or command
  execution. **Bypass skips approval prompts; it does not override policy denials.**
- Required isolation fails closed when unavailable. Permission settings alone
  are not an operating-system sandbox.
- Prompts and relevant context go to your configured model provider. Consider
  that provider's data handling when working with sensitive projects.
- Credential handling uses redacted secret types. Do not paste credentials into
  prompts, reports, or shared logs.
- Skills, plugins, MCP servers, and worker selection do not grant additional
  execution authority; operations remain subject to applicable permission checks.

## Updates and further information

Use the latest published release for available fixes. The native updater and
installers verify package checksums. Release publication requires the project's
CI gates, including dependency and advisory checks.

See [releases](https://github.com/99percentgrip/agent-vesper/releases),
[installation and updates](docs/installation.md),
[the user guide](docs/using-vesper.md), and
[architecture decisions](docs/adr/). Dependency policy is recorded in
[`deny.toml`](deny.toml).
