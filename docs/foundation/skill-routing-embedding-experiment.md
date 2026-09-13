# Optional semantic routing experiment

Date: 2026-09-13. Status: ongoing research; no native embedding route installed.
PRD: [Skill routing quality](../skill-routing-quality-prd.md).

## Objective and method

Test whether semantic retrieval closes the residual natural-language gap while
preserving metadata-only selection, policy gates and the complete skill library.
This is an optional D6 experiment, not native acceptance or an installed dependency.
The [lexical follow-up](skill-routing-language-execution.md) is implemented on this
isolated branch. A separately approved [model-assisted selector](skill-routing-model-assistance-execution.md)
now builds on its metadata shortlist; no embedding runtime is installed.

The first candidate is [all-MiniLM-L6-v2](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2),
revision `1110a243fdf4706b3f48f1d95db1a4f5529b4d41`, whose model card declares
Apache-2.0 and 384-dimensional normalized embeddings. The probe uses masked mean
pooling, local ONNX inference and cosine similarity. It refuses inputs exceeding
256 tokens rather than silently truncating. No training, provider call or skill
body is used. The ONNX SHA-256 from the pinned repository is
`6fd5d72fe4589f189f8ebc006442dbb529bb7ce38f8082112682524616046452`.

Python 3.14 lacked wheels for the initial older package pins; that install failed
and was replaced with binary-only compatible packages in `/tmp/vesper-routing-embedding`.
The exact successful package list is `/tmp/routing-embedding-packages.txt`.
Nothing was installed into Vesper or the user's skill library. One runtime-generated
runtime artifact appeared in the isolated checkout during the initial probe;
it was removed, telemetry was disabled, and subsequent probes run in the dedicated
experiment directory. No original-workspace or installed-user-state write occurred.

The Rust development diagnostic exports bounded `name`, `description`, `tags`,
`triggers` and file-extension metadata. The probe never reads skill source bodies.
Similarity cutoffs are selected on development labels only, maximizing positive
recall subject to zero negative activation, then fewer selected skills. The
original test corpus is already inspected regression data, not a fresh independent
holdout. Native policy/body-loading and 25/100/250/500 acceptance are not inferred
from a retrieval-only experiment on the 95-entry actual catalog.

## Results retained so far

[Original embedding predictions](skill-routing-embedding-results.json): development
34/40 positive recall, held-out-labelled 36/40, zero activation on each 20-query
negative split; threshold 0.38. Model load 82.353 ms, catalog encoding 780.430 ms,
query warm p95 3.214 ms (ten warm-ups, 100 measured queries); vector storage 291,840
bytes. The probe currently pools into float64 arrays; this byte count reflects
what it actually holds. Timing does not include native host/policy overhead.

A fixed two-lexical/one-semantic fusion yields 37/40 positive recall; equal reciprocal
rank fusion with constant 60 yields 38/40 on the held-out-labelled positives,
40/40 development positives, and zero negative activation. These do not establish
all PRD gates, especially the separate no-name subset and harmful sibling controls.
Neither fusion has been wired into native routing.

[Representation comparison](skill-routing-embedding-representations.json) predeclared
three variants: description only, identity plus description, and metadata including
tags. Development-only selection tied at 34/40; the fewer-selections tie break chose
a representation reaching only 28/40 on the held-out-labelled split. This regression
is retained; the worse result is not hidden or promoted.

The second predeclared candidate is [BGE small English v1.5](https://huggingface.co/BAAI/bge-small-en-v1.5),
revision `5c38ec7c405ec4b44b94cc5a9bb96e735b38267a`, with MIT-declared weights.
Its pinned pooling configuration specifies CLS pooling, 384 dimensions. Queries
use the publisher's retrieval prefix, and vectors are normalized. Its ONNX SHA-256
is `828e1496d7fabb79cfa4dcd84fa38625c0d3d21da474a00f08db0f559940cf35`.
The checksum-verified [initial BGE probe](skill-routing-bge-results.json) reached
30/40 development positives and 32/40 held-out-labelled positives, with zero
negative activations and a 0.67 development-selected threshold. Its 95-entry
catalog encoding took 1,659.826 ms and query warm p95 4.404 ms.

A follow-up used the native lexical index's request/overlap signal before semantic
selection. With the original hand-written verb list it reached 40/40 development
but only 22/40 held-out-labelled positives ([receipt](skill-routing-bge-gated-results.json)).
With independently sourced verb recognition it reached 40/40 development and
36/40 held-out-labelled positives, but activated on 1/20 negatives
([receipt](skill-routing-bge-verbs-results.json)). Those experiments confirm a
request-recognition gap and a semantic abstention regression; they do not justify
shipping a native embedding dependency. Current lexical development is addressing
request recognition independently of model or skill identity.

## Files, reproduction and boundaries

`skill-routing-embedding-probe.py` is an explicitly invoked non-production helper.
It accepts the local verified model directory, Rust metadata export, corpus JSON
and output JSON path. `routing_quality_development.rs` exports metadata only when
`VESPER_ROUTING_METADATA_OUTPUT` is explicitly provided. Prototype representation
and second-model probes currently reside in `/tmp/vesper-routing-embedding`.
The checked-in probe now supports both pinned models, fixed thresholds, bounded
representations and optional native policy/request flags; independent-corpus mode
requires an explicit fixed threshold and performs no calibration. The exact
[package freeze](skill-routing-embedding-packages.txt) is retained.

Quality adoption remains HOLD. No label, skill source, permission gate, GLM-owned
code, public release or local installation was changed by this experiment.


## Independent-corpus diagnostic

The fixed MiniLM 0.38 probe found an acceptable ID on 82/95 positives, abstained on
12/95, and activated on 9/60 no-skill cases; sibling recall was 28/30. The
[complete receipt](skill-routing-independent-minilm-results.json) is retrieval-only,
without native policy filtering, and does not pass adoption. Native-policy-filtered
original probes are retained in [MiniLM](skill-routing-embedding-policy-results.json)
and [BGE](skill-routing-bge-policy-results.json) receipts; policy filtering did not
establish the missing quality gates. No embedding runtime is promoted.
