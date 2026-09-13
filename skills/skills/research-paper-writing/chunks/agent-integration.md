<!-- chunk: Agent Integration -->

## the agent Integration

This skill is designed for the host agent. It uses the agent's hosted tools, delegation, scheduling, and memory for the full research lifecycle.

### Related Skills

Compose this skill with other library skills for specific phases:

| Skill | When to Use | How to Load |
|-------|-------------|-------------|
| **arxiv** | Phase 1 (Literature Review): searching arXiv, generating BibTeX, finding related papers via Semantic Scholar | `read_skill(name="arxiv")` |
| **plan** | Phase 0 (Setup): creating structured plans before execution. Writes to `.agent-vesper/plans/` | `read_skill(name="plan")` |
| **excalidraw** | Phase 4-5: hand-drawn-style figures and architecture diagrams | `read_skill(name="excalidraw")` |
| **arxiv** | Phase 1 (Literature Review): searching arXiv, BibTeX, related papers | `read_skill(name="arxiv")` |

### Agent Tools Reference

| Tool | Usage in This Pipeline |
|------|----------------------|
| **`bash`** | LaTeX compilation (`latexmk -pdf`), git operations, launching experiments (`nohup python run.py &`), process checks |
| **`process`** | Background experiment management: `process("start", ...)`, `process("poll", pid)`, `process("log", pid)`, `process("kill", pid)` |
| **`execute_code`** | Run Python for citation verification, statistical analysis, data aggregation. Has tool access via RPC. |
| **`read_file`** / **`write_file`** / **`apply_patch_set`** | Paper editing, experiment scripts, result files. Use `apply_patch_set` for targeted edits to large .tex files. |
| **`web_search`** | Literature discovery: `web_search("transformer attention mechanism 2024")` |
| **`web_reader`** | Fetch paper content, verify citations: `web_reader("https://arxiv.org/abs/2303.17651")` |
| **`delegate_task`** | **Parallel section drafting** — spawn isolated subagents for each section. Also for concurrent citation verification. |
| **`todo`** | Primary state tracker across sessions. Update after every phase transition. |
| **`memory`** | Persist key decisions across sessions: contribution framing, venue choice, reviewer feedback. |
| **`cronjob`** | Schedule experiment monitoring, deadline countdowns, automated arXiv checks. |
| **`clarify`** | Ask the user targeted questions when blocked (venue choice, contribution framing). |
| **`cronjob` delivery** | Notify the user when experiments complete or drafts are ready: schedule the check as a `cronjob` whose prompt reports results in the session. |

### Tool Usage Patterns

**Experiment monitoring** (most common):
```
bash("ps aux | grep <pattern>")
→ bash("tail -30 <logfile>")
→ terminal("ls results/")
→ execute_code("analyze results JSON, compute metrics")
→ terminal("git add -A && git commit -m '<descriptive message>' && git push")
→ (final response auto-delivers "Experiment complete: <summary>"; for unattended runs, schedule via cron with a deliver: target)
```

**Parallel section drafting** (using delegation):
```
delegate_task("Draft the Methods section based on these experiment scripts and configs. 
  Include: pseudocode, all hyperparameters, architectural details sufficient for 
  reproduction. Write in LaTeX using the neurips2025 template conventions.")

delegate_task("Draft the Related Work section. Use web_search and web_reader to 
  find papers. Verify every citation via Semantic Scholar. Group by methodology.")

delegate_task("Draft the Experiments section. Read all result files in results/. 
  State which claim each experiment supports. Include error bars and significance.")
```

Each delegate runs as a **fresh subagent** with no shared context — provide all necessary information in the prompt. Collect outputs and integrate.

**Citation verification** (using execute_code):
```python
# In execute_code:
from semanticscholar import SemanticScholar
import requests

sch = SemanticScholar()
results = sch.search_paper("attention mechanism transformers", limit=5)
for paper in results:
    doi = paper.externalIds.get('DOI', 'N/A')
    if doi != 'N/A':
        bibtex = requests.get(f"https://doi.org/{doi}", 
                              headers={"Accept": "application/x-bibtex"}).text
        print(bibtex)
```

### State Management with `memory` and `todo`

**`memory` tool** — persist key decisions (bounded: ~2200 chars for MEMORY.md):

```
memory("add", "Paper: autoreason. Venue: NeurIPS 2025 (9 pages). 
  Contribution: structured refinement works when generation-evaluation gap is wide.
  Key results: Haiku 42/42, Sonnet 3/5, S4.6 constrained 2/3.
  Status: Phase 5 — drafting Methods section.")
```

Update memory after major decisions or phase transitions. This persists across sessions.

**`todo` tool** — track granular progress:

```
todo("add", "Design constrained task experiments for Sonnet 4.6")
todo("add", "Run Haiku baseline comparison")
todo("add", "Draft Methods section")
todo("update", id=3, status="in_progress")
todo("update", id=1, status="completed")
```

**Session startup protocol:**
```
1. todo("list")                           # Check current task list
2. memory("read")                         # Recall key decisions
3. terminal("git log --oneline -10")      # Check recent commits
4. terminal("ps aux | grep python")       # Check running experiments
5. terminal("ls results/ | tail -20")     # Check for new results
6. Report status to user, ask for direction
```

### Cron Monitoring with `cronjob`

Use the `cronjob` tool to schedule periodic experiment checks:

```
cronjob("create", {
  "schedule": "*/30 * * * *",  # Every 30 minutes
  "prompt": "Check experiment status:
    1. ps aux | grep run_experiment
    2. tail -30 logs/experiment_haiku.log
    3. ls results/haiku_baselines/
    4. If complete: read results, compute Borda scores, 
       git add -A && git commit -m 'Add Haiku results' && git push
    5. Report: table of results, key finding, next step
    6. If nothing changed: respond with [SILENT]"
})
```

**[SILENT] protocol**: When nothing has changed since the last check, respond with exactly `[SILENT]`. This suppresses notification delivery to the user. Only report when there are genuine changes worth knowing about.

**Deadline tracking**:
```
cronjob("create", {
  "schedule": "0 9 * * *",  # Daily at 9am
  "prompt": "NeurIPS 2025 deadline: May 22. Today is {date}. 
    Days remaining: {compute}. 
    Check todo list — are we on track? 
    If <7 days: warn user about remaining tasks."
})
```

### Communication Patterns

**When to notify the user** (via your direct/final response, or a cron `deliver:` target for unattended runs):
- Experiment batch completed (with results table)
- Unexpected finding or failure requiring decision
- Draft section ready for review
- Deadline approaching with incomplete tasks

**When NOT to notify:**
- Experiment still running, no new results → `[SILENT]`
- Routine monitoring with no changes → `[SILENT]`
- Intermediate steps that don't need attention

**Report format** — always include structured data:
```
## Experiment: <name>
Status: Complete / Running / Failed

| Task | Method A | Method B | Method C |
|------|---------|---------|---------|
| Task 1 | 85.2 | 82.1 | **89.4** |

Key finding: <one sentence>
Next step: <what happens next>
```

### Decision Points Requiring Human Input

Use `clarify` for targeted questions when genuinely blocked:

| Decision | When to Ask |
|----------|-------------|
| Target venue | Before starting paper (affects page limits, framing) |
| Contribution framing | When multiple valid framings exist |
| Experiment priority | When TODO list has more experiments than time allows |
| Submission readiness | Before final submission |

**Do NOT ask about** (be proactive, make a choice, flag it):
- Word choice, section ordering
- Which specific results to highlight
- Citation completeness (draft with what you find, note gaps)

---
