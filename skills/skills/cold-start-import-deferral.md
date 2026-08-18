---
name: cold-start-import-deferral
description: Defer heavy module-top imports out of the legacy glm_acp Python CLI cold path so --version/--setup/--check-auth/--uninstall and the cron/plugin/observe/harden/meta-* commands stay fast. Use when cold start regresses or new top-level imports land in cli modules.
version: 1.1.0
author: Agent Vesper library (migrated from legacy GLM-ACP)
license: MIT
platforms: [linux, macos, windows]
metadata:
  vesper:
    tags: [cold-start, lazy-imports, performance, legacy]
prerequisites:
  commands: [uv]
---

# Cold Start Import Deferral (legacy Python agent)

Profile first: `uv run python -X importtime -c "from glm_acp import cli"
2>&1 | tail -30`. Target any chain over ~10 ms that is not needed by every
command.

Deferral pattern (per file):

1. Read the file. Confirm every use of the heavy symbol is INSIDE a
   function or method body, not at module top. With
   `from __future__ import annotations`, type annotations are lazy strings
   and do NOT need a runtime import.
2. Move the heavy `import`/`from` statement out of module top into the
   smallest function(s) that actually use it at runtime. Python caches
   modules, so repeated in-function imports are free after the first call.
3. For names that appear only in annotations, add a `if TYPE_CHECKING:`
   block at module top so ruff/mypy still resolve them:

       from typing import TYPE_CHECKING
       if TYPE_CHECKING:
           from .agent import GlmAcpAgent

4. Parser-registration functions (`add_chat_parser`, `add_cron_parser`,
   `add_plugin_parser`) must stay lightweight — they run on every CLI
   invocation including --version. Their heavy runtime imports belong
   inside `run_*_command`.

Regression test (in tests/test_cli.py): run in a FRESH SUBPROCESS because
pytest pollutes sys.modules:

    result = subprocess.run([sys.executable, "-c",
        "import sys; import glm_acp.cli; print([m for m in (...) if m in sys.modules])"],
        capture_output=True, text=True)
    assert result.stdout.strip() == "[]"

Currently-guarded heavy modules: acp, acp.schema, httpx, croniter,
cryptography, rich.console, glm_acp.agent, glm_acp.cron, glm_acp.plugins.

Verify after deferral: `uv run ruff check .` (catches F821 undefined-name
from annotations) and `uv run pytest -q`. Smoke-test every deferred path:
--version, --help, chat --help, cron --help, cron list, plugin --help,
plugin publishers, --check-auth.

## Provenance

Migrated from the legacy native-glm-acp (Python) learned-skill store.
