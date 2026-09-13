---
name: research-paper-writing
title: Research Paper Writing Pipeline
description: "Write ML papers for NeurIPS/ICML/ICLR: design→submit."
version: 1.2.0
author: Orchestra Research
license: MIT
dependencies: [semanticscholar, arxiv, habanero, requests, scipy, numpy, matplotlib, SciencePlots]
platforms: [linux, macos]
tags: [Research, Paper Writing, Experiments, ML, AI, NeurIPS, ICML, ICLR, ACL, AAAI, COLM, LaTeX, Citations, Statistical Analysis]
related_skills: [arxiv, plan]
chunks:
  - name: phase0-setup
    description: "Load when starting a paper project: exploring the repository, organizing the workspace, git setup, identifying the contribution, budgeting compute, coordinating co-authors."
    summary: "Workspace structure, contribution framing, TODO discipline, cost tracking, multi-author conventions."
    key_elements: [workspace, contribution, todo, budget, co-authors]
  - name: phase1-literature-review
    description: "Load when searching related work, identifying baselines, gathering and verifying citations, or organizing the related-work section."
    summary: "Breadth-then-depth search loops, citation verification ladder, BibTeX retrieval by DOI, grouping prior work by methodology."
    key_elements: [bibliography, baselines, bibtex, doi, citation, related-work]
  - name: phase2-experiment-design
    description: "Load when mapping claims to experiments, designing baselines, defining metrics and statistical protocol, or planning human evaluation."
    summary: "Claim-to-experiment mapping, baseline taxonomy, evaluation protocol, script scaffolding, annotator study design."
    key_elements: [claims, baselines, metrics, protocol, annotators]
  - name: phase3-execution-monitoring
    description: "Load while experiments run: launching background jobs, cron monitoring prompts, failure recovery, committing result batches, journaling."
    summary: "nohup launches, cron check prompts with silent mode, rate-limit recovery, incremental result saving, exploration-tree journal."
    key_elements: [nohup, cron, monitoring, crash-recovery, journal]
  - name: phase4-analysis
    description: "Load after experiments finish: aggregating results, error bars, significance tests, story identification, figures, experiment-log template."
    summary: "Aggregation scripts, confidence intervals, McNemar and Cohen statistics, narrative synthesis, log skeleton for writeup."
    key_elements: [aggregation, error-bars, significance, story, figures]
  - name: phase5-drafting
    description: "Load when drafting the paper itself: section order, LaTeX scaffolding, abstract and intro formulas, related-work positioning."
    summary: "Pointer to the full drafting procedure and prose style companion in the references layer."
    key_elements: [drafting, latex, abstract, intro]
  - name: phase6-review-revision
    description: "Load when simulating reviews or revising: ensemble reviewer prompts, meta-review aggregation, visual review pass, claim verification."
    summary: "Adversarial reviewer JSON prompts, area-chair aggregation, VLM visual pass, few-shot calibration, revision loops."
    key_elements: [reviewers, meta-review, revision, calibration]
  - name: phase7-submission
    description: "Load when preparing the camera-ready submission: venue checklist, reproduction package, citation export, anonymity and final build."
    summary: "Submission checklists, reproduction bundle, BibTeX export, build discipline for the deadline."
    key_elements: [submission, camera-ready, reproduction, deadline]
  - name: phase8-post-acceptance
    description: "Load after acceptance: preparing the camera-ready, posters, talks, code release, and audience-specific communication."
    summary: "Poster templates, talk structure, code release packaging, artifact documentation."
    key_elements: [poster, talk, code-release, camera-ready]
  - name: paper-types
    description: "Load when the paper is not a standard empirical ML study: theory, survey, benchmark, or position paper structures and evidence standards."
    summary: "Structural and evidentiary requirements per paper genre beyond empirical ML."
    key_elements: [theory, survey, benchmark, position]
  - name: workshop-short-papers
    description: "Load when targeting a workshop or four-page short paper: scope constraints, iteration speed, extended-version strategy."
    summary: "Workshop vs main-conference trade-offs, short-paper scoping, extension path."
    key_elements: [workshop, short-paper, four-page, scope]
  - name: iterative-refinement
    description: "Load when iterating drafts through generation-evaluation loops: autoreason strategy selection, convergence criteria, failure modes."
    summary: "Quick decision table, generation-evaluation gap analysis, convergence and drift diagnostics."
    key_elements: [autoreason, convergence, drift, judges]
  - name: agent-integration
    description: "Load when running the pipeline inside an autonomous agent loop: delegation patterns, parallel search rounds, structured logging."
    summary: "Agent-loop integration patterns, parallel delegate rounds, experiment snapshot template."
    key_elements: [delegation, parallel-rounds, experiment-log, snapshot]
  - name: reviewer-criteria
    description: "Load when calibrating simulated reviewers or answering rebuttals: soundness, clarity, significance, originality rubric."
    summary: "Four-axis reviewer rubric with scoring anchors."
    key_elements: [soundness, clarity, significance, originality]
  - name: common-issues
    description: "Load when the pipeline stalls: common pipeline problems and their fixes."
    summary: "Diagnostic table of recurring failure symptoms and remedies."
    key_elements: [stalls, symptoms, remedies, diagnostics]
  - name: reference-docs
    description: "Load when locating the deep-dive companions: index of the reference documents layer."
    summary: "Map of the ten reference documents and what each contains."
    key_elements: [companions, index, deep-dives]
---

## When To Use This Skill

Use this skill when:
- **Starting a new research paper** from an existing codebase or idea
- **Designing and running experiments** to support paper claims
- **Writing or revising** any section of a research paper
- **Preparing for submission** to a specific conference or workshop
- **Responding to reviews** with additional experiments or revisions
- **Converting** a paper between conference formats
- **Writing non-empirical papers** — theory, survey, benchmark, or position papers (see [Paper Types Beyond Empirical ML](#paper-types-beyond-empirical-ml))
- **Designing human evaluations** for NLP, HCI, or alignment research
- **Preparing post-acceptance deliverables** — posters, talks, code releases

## Core Philosophy

1. **Be proactive.** Deliver complete drafts, not questions. Scientists are busy — produce something concrete they can react to, then iterate.
2. **Never hallucinate citations.** AI-generated citations have ~40% error rate. Always fetch programmatically. Mark unverifiable citations as `[CITATION NEEDED]`.
3. **Paper is a story, not a collection of experiments.** Every paper needs one clear contribution stated in a single sentence. If you can't do that, the paper isn't ready.
4. **Experiments serve claims.** Every experiment must explicitly state which claim it supports. Never run experiments that don't connect to the paper's narrative.
5. **Commit early, commit often.** Every completed experiment batch, every paper draft update — commit with descriptive messages. Git log is the experiment history.

### Proactivity and Collaboration

**Default: Be proactive. Draft first, ask with the draft.**

| Confidence Level | Action |
|-----------------|--------|
| **High** (clear repo, obvious contribution) | Write full draft, deliver, iterate on feedback |
| **Medium** (some ambiguity) | Write draft with flagged uncertainties, continue |
| **Low** (major unknowns) | Ask 1-2 targeted questions via `clarify`, then draft |

| Section | Draft Autonomously? | Flag With Draft |
|---------|-------------------|-----------------|
| Abstract | Yes | "Framed contribution as X — adjust if needed" |
| Introduction | Yes | "Emphasized problem Y — correct if wrong" |
| Methods | Yes | "Included details A, B, C — add missing pieces" |
| Experiments | Yes | "Highlighted results 1, 2, 3 — reorder if needed" |
| Related Work | Yes | "Cited papers X, Y, Z — add any I missed" |

**Block for input only when**: target venue unclear, multiple contradictory framings, results seem incomplete, explicit request to review first.

## Phase Routing Map

| Phase / Topic | Chunk | Load When |
|---|---|---|
| 0 — Project Setup | `phase0-setup` | Starting a project: workspace, contribution, budget |
| 1 — Literature Review | `phase1-literature-review` | Searching related work, verifying citations |
| 2 — Experiment Design | `phase2-experiment-design` | Mapping claims to experiments, baselines |
| 3 — Execution & Monitoring | `phase3-execution-monitoring` | While experiments run |
| 4 — Result Analysis | `phase4-analysis` | After experiments: statistics, story, figures |
| 5 — Paper Drafting | `phase5-drafting` | Writing the paper sections |
| 6 — Self-Review & Revision | `phase6-review-revision` | Simulating reviews, revising |
| 7 — Submission | `phase7-submission` | Camera-ready and deadline prep |
| 8 — Post-Acceptance | `phase8-post-acceptance` | Poster, talk, code release |
| Paper types | `paper-types` | Theory/survey/benchmark/position papers |
| Workshop / short | `workshop-short-papers` | Four-page or workshop targets |
| Iterative refinement | `iterative-refinement` | Generation-evaluation loops |
| Agent integration | `agent-integration` | Running inside an agent loop |
| Reviewer rubric | `reviewer-criteria` | Reviewer calibration, rebuttals |
| Common issues | `common-issues` | Pipeline stalls |
| Reference index | `reference-docs` | Locating deep-dive companions |

Chunk names in the routing map are directly addressable: a prompt naming a
chunk routes to it deterministically (name-match dominates ranking). When no
chunk vocabulary is present, only this lean body loads.
