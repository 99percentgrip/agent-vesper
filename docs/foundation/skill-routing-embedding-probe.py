#!/usr/bin/env python3
"""Offline optional-model experiment; does not modify or implement native routing.
Inputs are a verified local model, a bounded Rust metadata export and frozen labels.
No network calls, model training, skill bodies or installed state are used.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import time

parser = argparse.ArgumentParser(description=__doc__)
for name in ('model_dir', 'metadata_path', 'corpus_path', 'output_path'):
    parser.add_argument(name, type=Path)
parser.add_argument('--threshold', type=float, help='Use a previously calibrated cutoff; never calibrate on independent cases')
parser.add_argument('--model', choices=['minilm', 'bge'], default='minilm')
parser.add_argument('--policy', type=Path, help='Optional native eligibility export')
parser.add_argument('--request-flags', type=Path, help='Optional native request/overlap flags')
parser.add_argument('--representation', choices=['full', 'description', 'identity'], default='full')
args = parser.parse_args()

import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer

model_dir, metadata_path, corpus_path, output_path = (args.model_dir, args.metadata_path, args.corpus_path, args.output_path)
models = {
    'minilm': ('sentence-transformers/all-MiniLM-L6-v2', '1110a243fdf4706b3f48f1d95db1a4f5529b4d41', '6fd5d72fe4589f189f8ebc006442dbb529bb7ce38f8082112682524616046452', 'be50c3628f2bf5bb5e3a7f17b1f74611b2561a3a27eeab05e5aa30f411572037'),
    'bge': ('BAAI/bge-small-en-v1.5', '5c38ec7c405ec4b44b94cc5a9bb96e735b38267a', '828e1496d7fabb79cfa4dcd84fa38625c0d3d21da474a00f08db0f559940cf35', 'd241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66'),
}
model_name, revision, expected, tokenizer_hash = models[args.model]
assert hashlib.sha256((model_dir/'model.onnx').read_bytes()).hexdigest() == expected
assert hashlib.sha256((model_dir/'tokenizer.json').read_bytes()).hexdigest() == tokenizer_hash
policies = json.loads(args.policy.read_text()) if args.policy else None
flags = json.loads(args.request_flags.read_text()) if args.request_flags else None
prefix = 'Represent this sentence for searching relevant passages: ' if args.model == 'bge' else ''
ort.disable_telemetry_events()
records = json.loads(metadata_path.read_text())
assert len(records) <= 500
texts = [' '.join([r['name'], r['description'], ' '.join(r['tags']), ' '.join(r['triggers']), ' '.join(r['extensions'])]) for r in records]
if args.representation == 'description': texts = [r['description'] for r in records]
if args.representation == 'identity': texts = [r['name']+' '+r['description'] for r in records]
assert all(len(t.encode()) <= 32768 for t in texts)
cases = json.loads(corpus_path.read_text())['cases']
if any(c.get('split') == 'independent_holdout' for c in cases):
    assert args.threshold is not None, 'independent evaluation requires a fixed pre-calibrated threshold'
    cases = [dict(c, category=c['kind'], acceptable=c['expected_skill_ids']) for c in cases]
else:
    cases = [c for c in cases if c['category'] in ['positive', 'no_skill']]
tokenizer = Tokenizer.from_file(str(model_dir/'tokenizer.json'))
tokenizer.enable_padding()
options = ort.SessionOptions()
options.intra_op_num_threads = 2
options.inter_op_num_threads = 1
started = time.perf_counter()
session = ort.InferenceSession(str(model_dir/'model.onnx'), sess_options=options, providers=['CPUExecutionProvider'])
load_ms = (time.perf_counter()-started)*1000
inputs = {i.name for i in session.get_inputs()}

def encode(texts):
    batch = tokenizer.encode_batch(texts)
    assert max(len(b.ids) for b in batch) <= 256, 'probe refuses silent truncation'
    data = {'input_ids': np.array([b.ids for b in batch], dtype=np.int64), 'attention_mask': np.array([b.attention_mask for b in batch], dtype=np.int64), 'token_type_ids': np.array([b.type_ids for b in batch], dtype=np.int64)}
    values = session.run(None, {k:v for k,v in data.items() if k in inputs})[0]
    mask = data['attention_mask'][..., None]
    pooled = values[:, 0] if args.model == 'bge' else (values*mask).sum(1)/mask.sum(1).clip(min=1)
    return pooled/np.linalg.norm(pooled, axis=1, keepdims=True).clip(min=1e-12)

started = time.perf_counter()
vectors = np.concatenate([encode(texts[i:i+16]) for i in range(0,len(texts),16)])
index_ms = (time.perf_counter()-started)*1000
queries = np.concatenate([encode([prefix+c['prompt'] for c in cases[i:i+16]]) for i in range(0,len(cases),16)])
scores = queries@vectors.T
for i,c in enumerate(cases):
    for j,r in enumerate(records):
        if (policies is not None and r['slug'] not in policies[c['id']]) or (flags is not None and not flags[c['id']]):
            scores[i,j] = -1.0
# Calibration uses only development labels. Prefer recall, then fewer selections.
# Held-out labels never participate in the threshold choice.
choices=[]
for threshold in ([] if args.threshold is not None else (np.arange(.3,.901,.01) if args.model == 'bge' else np.arange(.15,.701,.01))):
    hits=negatives=selected_count=0
    for i,c in enumerate(cases):
        if c['split']!='development': continue
        selected=[records[j]['slug'] for j in np.argsort(-scores[i])[:3] if scores[i,j]>=threshold]
        selected_count+=len(selected)
        hits+=int(bool(set(selected)&set(c['acceptable'])))
        negatives+=int(c['category']=='no_skill' and bool(selected))
    if negatives==0: choices.append((hits,-selected_count,float(threshold)))
assert args.threshold is not None or choices, 'no valid development threshold'
threshold=args.threshold if args.threshold is not None else max(choices)[2]
assert 0 <= threshold <= 1
predictions=[]
summary={}
for i,c in enumerate(cases):
    order=np.argsort(-scores[i])[:3]
    selected=[records[j]['slug'] for j in order if scores[i,j]>=threshold]
    predictions.append({'id':c['id'],'split':c['split'],'selected':selected,'top3':[{'slug':records[j]['slug'],'cosine':float(scores[i,j])} for j in order]})
    counts=summary.setdefault(c['split']+':'+c['category'], {'n':0,'hits':0,'abstentions':0,'activations':0})
    counts['n']+=1
    counts['hits']+=int(bool(set(selected)&set(c['acceptable'])))
    counts['abstentions']+=int(not selected)
    counts['activations']+=int(bool(selected))
for _ in range(10): encode([prefix+cases[0]['prompt']])
times=[]
for _ in range(100):
    started=time.perf_counter();encode([prefix+cases[0]['prompt']]);times.append((time.perf_counter()-started)*1000)
result={'status':'retrieval-only experiment; native contracts and promotion not established','model':model_name,'revision':revision,'representation':args.representation,'corpus_sha256':hashlib.sha256(corpus_path.read_bytes()).hexdigest(),'policy_sha256':hashlib.sha256(args.policy.read_bytes()).hexdigest() if args.policy else None,'request_flags_sha256':hashlib.sha256(args.request_flags.read_bytes()).hexdigest() if args.request_flags else None,'model_sha256':expected,'tokenizer_sha256':hashlib.sha256((model_dir/'tokenizer.json').read_bytes()).hexdigest(),'metadata_sha256':hashlib.sha256(metadata_path.read_bytes()).hexdigest(),'dimensions':int(vectors.shape[1]),'model_load_ms':load_ms,'catalog_encode_ms':index_ms,'query_warm_p95_ms':sorted(times)[94],'vector_index_bytes':int(vectors.nbytes),'development_threshold':threshold,'threshold_source':'fixed caller-supplied' if args.threshold is not None else 'development calibration','summary':summary,'predictions':predictions}
output_path.write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:v for k,v in result.items() if k!='predictions'},indent=2))
