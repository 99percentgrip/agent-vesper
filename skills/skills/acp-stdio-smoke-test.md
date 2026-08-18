---
name: acp-stdio-smoke-test
description: Smoke-test the bare legacy `glm-acp` Python ACP server over stdio; both stdin AND stdout must be real pipes or asyncio raises ValueError. Use when verifying the legacy launch path after agent.py/cli.py changes.
version: 1.1.0
author: Agent Vesper library (migrated from legacy GLM-ACP)
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [acp, stdio, smoke-test, verification, legacy]
prerequisites:
  commands: [uv]
---

# ACP Stdio Smoke Test (legacy Python agent)

The bare `glm-acp` launch (no subcommand) starts an ACP JSON-RPC server over
stdio via `acp.run_agent` → `stdio_streams` → `loop.connect_read_pipe`/
`connect_write_pipe`. asyncio REFUSES regular files: redirecting either side
with `< file` or `> file` raises `ValueError: Pipe transport is for
pipes/sockets only` (read) or `Pipe transport is only for pipes, sockets and
character devices` (write). The traceback looks like an agent bug but is
purely a test-harness artifact.

Correct pattern — chain BOTH stdin and stdout through pipes:

    (printf '%s\n' \
      '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientInfo":{"name":"smoke","version":"0.0.1"}}}' \
      '{"jsonrpc":"2.0","method":"notifications/initialized"}'; \
     sleep 5) | timeout 10 uv run glm-acp 2>/dev/null | head -3

`clientInfo.version` is required by acp's pydantic schema — omitting it
returns a clean JSON-RPC -32602 error (still proves the stack works).

A successful initialize returns `{"jsonrpc":"2.0","id":1,"result":{"agentInfo":
{"name":"glm-acp","version":"..."},"agentCapabilities":{...},"authMethods":
[{"id":"zai-api-key-setup",...}],"protocolVersion":1}}`.

Never use `> out.txt` or `< in.txt` for the bare launch — only pipes (`|`)
or FIFOs work.

## Provenance

Migrated from the legacy native-glm-acp (Python) learned-skill store.
