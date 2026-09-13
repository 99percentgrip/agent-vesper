#!/usr/bin/env python3
"""Summarize frozen offline predictions; never tune labels or router parameters."""
import json
import math
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[2]
cases = {c['id']: c for c in json.loads((root/'crates/vesper-memory/tests/routing_quality_cases.json').read_text())['cases']}
results = json.loads(Path(sys.argv[1]).read_text())

def wilson(k, n):
    if not n:
        return 'unavailable'
    p = k/n
    z = 1.96
    center = (p+z*z/(2*n))/(1+z*z/n)
    half = z*math.sqrt(p*(1-p)/n+z*z/(4*n*n))/(1+z*z/n)
    return f'{100*(center-half):.1f}–{100*(center+half):.1f}%'

print('| Entries | Standard Recall@3 | Enhanced Recall@3 (95% Wilson interval) | Enhanced no-skill activation | Harmful sibling exposure |')
print('| --- | --- | --- | --- | --- |')
for size in [25, 100, 250, 500]:
    standard = results['summary'][f'{size}:Standard:held_out:positive']
    enhanced = results['summary'][f'{size}:Enhanced:held_out:positive']
    negative = results['summary'][f'{size}:Enhanced:held_out:no_skill']
    sibling = results['summary'][f'{size}:Enhanced:held_out:sibling']
    print(f'| {size} | {standard[2]}/{standard[0]} | {enhanced[2]}/{enhanced[0]} ({wilson(enhanced[2], enhanced[0])}) | {negative[4]}/{negative[0]} | {sibling[5]}/{sibling[0]} |')

print('\nHeld-out 500-entry positive families:')
family = {}
unnamed = []
failures = []
for prediction in results['predictions']:
    if prediction['size'] != 500 or prediction['mode'] != 'Enhanced' or prediction['split'] != 'held_out':
        continue
    case = cases[prediction['id']]
    if case['category'] == 'positive':
        hit = bool(set(case['acceptable']) & set(prediction['selected']))
        counts = family.setdefault(case['family'], [0, 0])
        counts[0] += int(hit)
        counts[1] += 1
        words = set(re.split(r'[^\w-]+', case['prompt'].lower()))
        if not any(slug in words for slug in case['acceptable']):
            unnamed.append((hit, not prediction['selected']))
        if not hit:
            failures.append(case['id'])
for name, (hits, n) in sorted(family.items()):
    print(f'- {name}: {hits}/{n}')
print(f'No-literal-name subset: hits={sum(h for h,_ in unnamed)}/{len(unnamed)}; abstentions={sum(a for _,a in unnamed)}/{len(unnamed)}')
print('Positive misses: '+', '.join(failures))
print('No-skill false activations: '+', '.join(p['id'] for p in results['predictions'] if p['size']==500 and p['mode']=='Enhanced' and p['split']=='held_out' and cases[p['id']]['category']=='no_skill' and p['selected']))
failed = []
for size in [25, 100, 250, 500]:
    positive = results['summary'][f'{size}:Enhanced:held_out:positive']
    negative = results['summary'][f'{size}:Enhanced:held_out:no_skill']
    sibling = results['summary'][f'{size}:Enhanced:held_out:sibling']
    if positive[2]/positive[0] < .95 or positive[3]/positive[0] > .05 or negative[4]/negative[0] > .02 or sibling[5]/sibling[0] > .02:
        failed.append(size)
print('\nVerdict: '+('HOLD: required quality rates fail at sizes '+str(failed) if failed else 'Rate checks pass; other PRD gates still require review')+'. This is not a downstream task-success benchmark.')
