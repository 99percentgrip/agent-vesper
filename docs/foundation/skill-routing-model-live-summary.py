#!/usr/bin/env python3
"""Score native live-routing JSONL receipts without changing labels or calling a provider.

Default refuses incomplete runs. --allow-partial produces explicitly provisional
coverage and metrics; never combines a partial trailing JSON line with a result.
Usage: python3 skill-routing-model-live-summary.py RECEIPT.jsonl [--output FILE.json]
"""
import argparse
import collections
import hashlib
import json
import math
from pathlib import Path

FROZEN_SHA = '79ef89a4934a0e2c61aa08d4e2de6a3efd0272c18f1ee3e9507271d5c6111275'
DEFAULT_CORPUS = Path(__file__).with_name('skill-routing-independent-corpus.json')
USAGE_FIELDS = ('input', 'output', 'total', 'reasoning', 'cached_input',
                'cache_write', 'audio', 'image', 'tool')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def string_list(value, field):
    require(isinstance(value, list) and all(isinstance(x, str) for x in value),
            f'{field} must be a list of strings')
    return value


def score(case, selected):
    """Frozen set-of-acceptable-IDs contract, including required empty outcomes."""
    selected = string_list(selected, 'selected')
    actual, acceptable = set(selected), set(case['expected_skill_ids'])
    shape_ok = len(actual) == len(selected) and len(selected) <= 3
    hit = bool(actual & acceptable)
    extra = bool(actual - acceptable)
    forbidden = bool(actual & set(case['forbidden_skill_ids']))
    required = (hit or (case['acceptable_abstention'] and not actual)) if acceptable else not actual
    return dict(hit=hit, abstained=not actual, extra_activation=extra,
                forbidden_activation=forbidden, valid_selection_shape=shape_ok,
                strict_label_satisfied=bool(shape_ok and required and not extra and not forbidden))


def latency(values):
    values = sorted(values)
    if not values:
        return dict(n=0, sum_ms=0, min_ms=None, mean_ms=None, p50_ms=None,
                    p95_ms=None, p99_ms=None, max_ms=None)
    quantile = lambda p: values[max(0, math.ceil(p * len(values)) - 1)]
    return dict(n=len(values), sum_ms=sum(values), min_ms=values[0],
                mean_ms=sum(values)/len(values), p50_ms=quantile(.50),
                p95_ms=quantile(.95), p99_ms=quantile(.99), max_ms=values[-1])


def proportion(successes, n):
    """Wilson score interval; descriptive binomial model, not traffic sampling."""
    if not n:
        return dict(count=successes, n=n, rate=None, wilson95_low=None, wilson95_high=None)
    z = 1.959963984540054
    p = successes / n
    denominator = 1 + z*z/n
    centre = (p + z*z/(2*n)) / denominator
    half = z * math.sqrt(p*(1-p)/n + z*z/(4*n*n)) / denominator
    return dict(count=successes, n=n, rate=p,
                wilson95_low=max(0.0, centre-half), wilson95_high=min(1.0, centre+half))


def baseline_comparison(path, cases, records):
    payload = path.read_bytes()
    baseline = json.loads(payload)
    require(baseline.get('corpus_sha256') == FROZEN_SHA, 'baseline corpus digest mismatch')
    standard, enhanced = {}, {}
    for row in baseline['predictions']:
        if row['mode'] not in ('Standard', 'Enhanced'):
            continue
        target = standard if row['mode'] == 'Standard' else enhanced
        require(row['id'] in cases and row['id'] not in target, 'invalid/duplicate baseline ID')
        if row['mode'] == 'Enhanced':
            require(isinstance(row.get('expected_rejections'), list),
                    'Enhanced baseline must explicitly record expected_rejections')
        target[row['id']] = row
    require(set(standard) == set(cases), 'Standard baseline must cover all 205 frozen cases')
    comparisons = {}
    for name, kind, field in [('positive_recall', 'positive', 'hit'),
                              ('positive_strict_success', 'positive', 'strict_label_satisfied'),
                              ('no_skill_false_activation', 'no_skill', 'extra_activation'),
                              ('sibling_forbidden_exposure', 'sibling', 'forbidden_activation')]:
        cohort = [r for r in records if r['kind'] == kind]
        live_count = sum(r['final_score'][field] for r in cohort)
        base_count = sum(score(cases[r['id']], standard[r['id']]['selected'])[field] for r in cohort)
        comparisons[name] = dict(live=proportion(live_count, len(cohort)),
                                 current_standard=proportion(base_count, len(cohort)),
                                 delta_percentage_points=(100*(live_count-base_count)/len(cohort)
                                                          if cohort else None))
    positive = [r for r in records if r['kind'] == 'positive']
    diagnosed = [r for r in positive if r['id'] in enhanced]
    eligible = [r for r in diagnosed if not enhanced[r['id']].get('expected_rejections')]
    blocked = [dict(id=r['id'], expected_skill_ids=r['expected_skill_ids'],
                    rejections=enhanced[r['id']].get('expected_rejections'))
               for r in diagnosed if enhanced[r['id']].get('expected_rejections')]
    return dict(path=str(path), sha256=hashlib.sha256(payload).hexdigest(),
                comparison_scope='Same observed frozen case IDs; complete denominators only when full live coverage exists.',
                comparisons=comparisons,
                positive_eligibility_diagnostic=dict(
                    authority='All 95 positive labels remain authoritative; filtered rows are diagnostic only.',
                    basis='Expected-target rejections in current native Enhanced receipt; not inferred from live shortlist absence.',
                    labelled_n=len(positive), baseline_diagnosed_n=len(diagnosed),
                    without_recorded_rejection_n=len(eligible),
                    live_recall=proportion(sum(r['final_score']['hit'] for r in eligible),len(eligible)),
                    live_strict=proportion(sum(r['final_score']['strict_label_satisfied'] for r in eligible),len(eligible)),
                    blocked=blocked))


def load_receipts(path, allow_partial):
    payload = path.read_bytes()
    lines = payload.splitlines(keepends=True)
    rows, ignored_tail = [], False
    for number, line in enumerate(lines, 1):
        if not line.endswith(b'\n'):
            require(allow_partial and number == len(lines),
                    f'receipt line {number} is not a completed newline-terminated record')
            ignored_tail = True
            break
        require(line.strip(), f'blank receipt line {number}')
        try:
            row = json.loads(line)
        except (ValueError, UnicodeError) as error:
            raise ValueError(f'invalid JSON on completed line {number}') from error
        require(isinstance(row, dict), f'receipt line {number} must be an object')
        rows.append(row)
    require(rows and 'corpus_sha256' in rows[0] and 'id' not in rows[0], 'missing receipt header')
    return payload, rows[0], rows[1:], ignored_tail


def summarize(corpus_path, receipt_path, allow_partial=False, baseline_path=None):
    corpus_bytes = corpus_path.read_bytes()
    digest = hashlib.sha256(corpus_bytes).hexdigest()
    require(digest == FROZEN_SHA, 'corpus is not the frozen independently authored corpus')
    corpus = json.loads(corpus_bytes)
    cases = {case['id']: case for case in corpus['cases']}
    require(len(cases) == len(corpus['cases']) == 205, 'corpus ID/count mismatch')
    raw, header, rows, ignored_tail = load_receipts(receipt_path, allow_partial)
    require(header['corpus_sha256'] == digest, 'receipt/corpus digest mismatch')
    require(header.get('cases') == len(cases), 'receipt header case count mismatch')
    seen, records = set(), []
    strata = collections.defaultdict(collections.Counter)
    statuses, errors, warnings = collections.Counter(), collections.Counter(), []
    latency_groups = collections.defaultdict(list)
    usage_modes = collections.Counter()
    usage = {field: dict(cumulative_reported_sum=0, cumulative_reported_rows=0,
                        unavailable_rows=0, noncumulative_rows=0, provenance_counts={})
             for field in USAGE_FIELDS}
    usage_receipts = 0
    for row in rows:
        case_id = row.get('id')
        require(case_id in cases, f'unknown receipt case ID: {case_id}')
        require(case_id not in seen, f'duplicate receipt case ID: {case_id}')
        seen.add(case_id)
        case = cases[case_id]
        for field in ('selected', 'offered', 'unsupported_tools', 'unsupported_permissions'):
            string_list(row.get(field), field)
        require(len(row['offered']) <= 12 and len(set(row['offered'])) == len(row['offered']),
                f'{case_id}: invalid shortlist bound or duplicate offer')
        declared = case['context']
        require(set(row['unsupported_permissions']) == set(declared['denied_permissions']),
                f'{case_id}: unsupported permission coverage differs from frozen labels')
        require(set(row['unsupported_tools']) <= set(declared['unavailable_tools']),
                f'{case_id}: unsupported tools not declared by frozen case')
        # This native receipt does not carry positive proof of executor/permission
        # mapping. Never promote a constrained case because its row omits a hint.
        has_constraints = bool(declared['unavailable_tools'] or declared['denied_permissions'])
        context_supported = not (has_constraints or row['unsupported_tools'] or row['unsupported_permissions'])
        final = score(case, row['selected'])
        shortlist_hit = bool(set(case['expected_skill_ids']) & set(row['offered']))
        rank = next((i+1 for i, skill in enumerate(row['offered'])
                     if skill in case['expected_skill_ids']), None)
        selector = row.get('selector')
        raw_decision, raw_score, elapsed, error = None, None, None, None
        if selector is None:
            status = ('lexical_fallback_without_selector'
                      if 'lexical routing used' in row.get('reason', '')
                      or 'Standard routing used' in row.get('reason', '')
                      else 'no_selector_prepared')
        else:
            require(isinstance(selector, dict), f'{case_id}: selector must be object/null')
            elapsed = selector.get('elapsed_ms')
            require(isinstance(elapsed, int) and not isinstance(elapsed, bool) and elapsed >= 0,
                    f'{case_id}: invalid elapsed_ms')
            decision = selector.get('decision')
            require(isinstance(decision, dict) and set(decision) in ({'Ok'}, {'Err'}),
                    f'{case_id}: malformed Result decision')
            if 'Ok' in decision:
                raw_decision = decision['Ok']
                require(isinstance(raw_decision, dict) and set(raw_decision) == {'outcome', 'skills'},
                        f'{case_id}: malformed successful decision')
                skills = string_list(raw_decision['skills'], 'decision.skills')
                require(raw_decision['outcome'] in ('selected', 'no_skill_needed', 'ambiguous'),
                        f'{case_id}: invalid decision outcome')
                require((raw_decision['outcome'] == 'selected') == bool(skills),
                        f'{case_id}: decision outcome/IDs contradict')
                require(len(skills) <= 3 and len(set(skills)) == len(skills)
                        and set(skills) <= set(row['offered']),
                        f'{case_id}: model decision violates shortlist/schema contract')
                raw_score = score(case, skills)
                status = ('validated_model_decision' if row['offered']
                          else 'deterministic_empty_shortlist_no_call')
            else:
                error = decision['Err']
                require(isinstance(error, str), f'{case_id}: error must be string')
                errors[error] += 1
                status = 'selector_error_lexical_fallback'
                if 'lexical routing used' not in row.get('reason', ''):
                    status = 'selector_error_without_lexical_fallback'
            latency_groups['all_selector_receipts'].append(elapsed)
            latency_groups[status].append(elapsed)
            # Nonempty offers alone do not prove an HTTP request was dispatched:
            # context/session-start errors can happen before a provider turn.
            if row['offered']:
                latency_groups['nonempty_shortlist_selector_path'].append(elapsed)
            snapshot = selector.get('usage')
            if snapshot is not None:
                require(isinstance(snapshot, dict), f'{case_id}: usage must be object/null')
                usage_receipts += 1
                mode = snapshot.get('mode', 'unknown')
                usage_modes[mode] += 1
                for field in USAGE_FIELDS:
                    counter = usage[field]
                    item = snapshot.get(field, {})
                    value, provenance = item.get('value'), item.get('provenance', 'unavailable')
                    counter['provenance_counts'][provenance] = counter['provenance_counts'].get(provenance, 0) + 1
                    if value is None:
                        counter['unavailable_rows'] += 1
                    elif mode != 'cumulative':
                        counter['noncumulative_rows'] += 1
                    else:
                        require(isinstance(value, (int, float)) and not isinstance(value, bool)
                                and math.isfinite(value) and value >= 0, f'{case_id}: invalid usage value')
                        counter['cumulative_reported_sum'] += value
                        counter['cumulative_reported_rows'] += 1
        statuses[status] += 1
        metrics = strata[case['kind']]
        metrics['n'] += 1
        for key in ('hit', 'abstained', 'extra_activation', 'forbidden_activation', 'strict_label_satisfied'):
            metrics[key] += int(final[key])
        metrics['context_unsupported'] += int(not context_supported)
        metrics['strict_supported_success'] += int(context_supported and final['strict_label_satisfied'])
        metrics['expected_in_shortlist'] += int(shortlist_hit)
        metrics['actual_model_decisions'] += int(status == 'validated_model_decision')
        metrics['model_decision_strict_success'] += int(status == 'validated_model_decision' and raw_score['strict_label_satisfied'])
        metrics['strict_supported_model_success'] += int(status == 'validated_model_decision' and context_supported and final['strict_label_satisfied'])
        metrics['fallbacks'] += int('fallback' in status)
        metrics['deterministic_empty_shortlist'] += int(status == 'deterministic_empty_shortlist_no_call')
        metrics['model_when_expected_offered_n'] += int(status == 'validated_model_decision' and shortlist_hit)
        metrics['model_when_expected_offered_strict'] += int(status == 'validated_model_decision' and shortlist_hit and raw_score['strict_label_satisfied'])
        records.append(dict(id=case_id, kind=case['kind'], clarity=case['clarity'],
                            expected_skill_ids=case['expected_skill_ids'], selected=row['selected'],
                            offered=row['offered'], expected_shortlist_rank=rank,
                            expected_in_shortlist=shortlist_hit, context_supported=context_supported,
                            context_constraints=declared, unsupported_tools=row['unsupported_tools'],
                            unsupported_permissions=row['unsupported_permissions'], selector_status=status,
                            model_decision=raw_decision, model_score=raw_score, final_score=final,
                            elapsed_ms=elapsed, selector_error=error, reason=row.get('reason', '')))
    missing = sorted(set(cases)-seen)
    complete = not missing and not ignored_tail
    require(allow_partial or complete, f'incomplete run: {len(seen)}/205 rows; use --allow-partial for provisional output')
    if any(mode != 'cumulative' for mode in usage_modes):
        warnings.append('Noncumulative usage snapshots are not summed as complete request usage.')
    warnings.extend([
        'All results are inspected-corpus regression evidence, not a fresh independent holdout.',
        'Shortlist absence conflates retrieval and native eligibility; this receipt has no target-rejection detail.',
        'Constrained resource cases receive no supported-pass credit without native mapping evidence.',
        'Selector-path attempts and elapsed time do not establish exact HTTP-call count or end-to-end wall time.',
        'Usage fields overlap: reasoning is not added to output, nor cached_input to input; no monetary cost is inferred.',
        '205 complete rows establish case coverage, not process exit success or library before/after invariance; inspect the runner completion log separately.',
        'Wilson intervals use a descriptive binomial model on a curated, nonrandom and potentially correlated corpus; they are not production-traffic generalization intervals.',
    ])
    intervals = {}
    for name, kind, field in [('positive_recall', 'positive', 'hit'),
                              ('positive_strict_success', 'positive', 'strict_label_satisfied'),
                              ('no_skill_false_activation', 'no_skill', 'extra_activation'),
                              ('sibling_forbidden_exposure', 'sibling', 'forbidden_activation')]:
        cohort = [r for r in records if r['kind'] == kind]
        intervals[name] = proportion(sum(r['final_score'][field] for r in cohort),len(cohort))
    result = dict(schema_version=1, status='complete_case_coverage' if complete else 'PROVISIONAL_PARTIAL',
                corpus_sha256=digest, receipt_sha256=hashlib.sha256(raw).hexdigest(),
                receipt_bytes=len(raw), receipt_header=header,
                coverage=dict(expected=205, observed=len(seen), missing_ids=missing,
                              ignored_incomplete_trailing_record=ignored_tail),
                totals=dict(statuses), selector_errors=dict(errors),
                by_kind={key:dict(value) for key,value in sorted(strata.items())},
                latency_method='nearest-rank quantiles; milliseconds',
                latency={key:latency(value) for key,value in sorted(latency_groups.items())},
                usage=dict(receipts_with_usage=usage_receipts,
                           selector_receipts_without_usage=len(latency_groups['all_selector_receipts'])-usage_receipts,
                           modes=dict(usage_modes), fields=usage,
                           unit='provider-reported normalized units; no price estimate'),
                failed_case_ids=[r['id'] for r in records if not r['final_score']['strict_label_satisfied']],
                unsupported_case_ids=[r['id'] for r in records if not r['context_supported']],
                wilson95=intervals,
                warnings=warnings, records=records)
    if baseline_path:
        result['standard_baseline_comparison'] = baseline_comparison(baseline_path,cases,records)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('receipt', type=Path)
    parser.add_argument('--corpus', type=Path, default=DEFAULT_CORPUS)
    parser.add_argument('--allow-partial', action='store_true')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--baseline', type=Path, help='Current native receipt containing all Standard cases and optional Enhanced eligibility rejections')
    args = parser.parse_args()
    try:
        result = summarize(args.corpus, args.receipt, args.allow_partial, args.baseline)
    except (ValueError, OSError, KeyError, TypeError) as error:
        parser.exit(2, f'Cannot score receipt: {error}\n')
    rendered = json.dumps(result, indent=2, ensure_ascii=False) + '\n'
    if args.output:
        require(args.output.resolve() not in (args.receipt.resolve(), args.corpus.resolve()),
                'output may not overwrite receipt or frozen corpus')
        args.output.write_text(rendered)
        print(json.dumps({key:result[key] for key in ('status','coverage','totals','by_kind','latency','usage')}, indent=2))
    else:
        print(rendered, end='')


if __name__ == '__main__':
    main()
