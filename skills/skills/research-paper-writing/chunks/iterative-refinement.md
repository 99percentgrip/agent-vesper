<!-- chunk: Iterative Refinement: Strategy Selection -->

## Iterative Refinement: Strategy Selection

Any output in this pipeline — paper drafts, experiment scripts, analysis — can be iteratively refined. The autoreason research provides empirical evidence for when each refinement strategy works and when it fails. Use this section to choose the right approach.

### Quick Decision Table

| Your Situation | Strategy | Why |
|---------------|----------|-----|
| Mid-tier model + constrained task | **Autoreason** | Sweet spot. Generation-evaluation gap is widest. Baselines actively destroy weak model outputs. |
| Mid-tier model + open task | **Autoreason** with scope constraints added | Add fixed facts, structure, or deliverable to bound the improvement space. |
| Frontier model + constrained task | **Autoreason** | Wins 2/3 constrained tasks even at frontier. |
| Frontier model + unconstrained task | **Critique-and-revise** or **single pass** | Autoreason comes last. Model self-evaluates well enough. |
| Concrete technical task (system design) | **Critique-and-revise** | Direct find-and-fix loop is more efficient. |
| Template-filling task (one correct structure) | **Single pass** or **conservative** | Minimal decision space. Iteration adds no value. |
| Code with test cases | **Autoreason (code variant)** | Structured analysis of *why* it failed before fixing. Recovery rate 62% vs 43%. |
| Very weak model (Llama 8B class) | **Single pass** | Model too weak for diverse candidates. Invest in generation quality. |

### The Generation-Evaluation Gap

**Core insight**: Autoreason's value depends on the gap between a model's generation capability and its self-evaluation capability.

```
Model Tier        │ Generation │ Self-Eval │ Gap    │ Autoreason Value
──────────────────┼────────────┼───────────┼────────┼─────────────────
Weak (Llama 8B)   │ Poor       │ Poor      │ Small  │ None — can't generate diverse candidates
Mid (Haiku 3.5)   │ Decent     │ Poor      │ LARGE  │ MAXIMUM — 42/42 perfect Borda
Mid (Gemini Flash)│ Decent     │ Moderate  │ Large  │ High — wins 2/3
Strong (Sonnet 4) │ Good       │ Decent    │ Medium │ Moderate — wins 3/5
Frontier (S4.6)   │ Excellent  │ Good      │ Small  │ Only with constraints
```

This gap is structural, not temporary. As costs drop, today's frontier becomes tomorrow's mid-tier. The sweet spot moves but never disappears.

### Autoreason Loop (Summary)

Each pass produces three candidates from fresh, isolated agents:

1. **Critic** → finds problems in incumbent A (no fixes)
2. **Author B** → revises A based on critique
3. **Synthesizer** → merges A and B (randomized labels)
4. **Judge Panel** → 3 blind CoT judges rank A, B, AB via Borda count
5. **Convergence** → A wins k=2 consecutive passes → done

**Key parameters:**
- k=2 convergence (k=1 premature, k=3 too expensive, no quality gain)
- CoT judges always (3x faster convergence)
- Temperature 0.8 authors, 0.3 judges
- Conservative tiebreak: incumbent wins ties
- Every role is a fresh agent with no shared context

### Applying to Paper Drafts

When refining the paper itself through autoreason:
- **Provide ground truth to the critic**: actual experimental data, result JSONs, statistical outputs. Without this, models hallucinate fabricated ablation studies and fake confidence intervals.
- **Use 3 working judges minimum**: A broken judge parser doesn't add noise — it prevents equilibrium entirely.
- **Scope constrain the revision**: "Address these specific weaknesses" not "improve the paper."

### Failure Modes

| Failure | Detection | Fix |
|---------|-----------|-----|
| No convergence (A never wins) | A wins <15% over 20+ passes | Add scope constraints to the task |
| Synthesis drift | Word counts grow unboundedly | Constrain structure and deliverable |
| Degradation below single pass | Baselines score higher than iterated output | Switch to single pass; model may be too weak |
| Overfitting (code) | High public-test pass, low private-test pass | Use structured analysis, not just test feedback |
| Broken judges | Parsing failures reduce panel below 3 | Fix parser before continuing |

See [references/autoreason-methodology.md](references/autoreason-methodology.md) for complete prompts, Borda scoring details, model selection guide, scope constraint design patterns, and compute budget reference.

---
