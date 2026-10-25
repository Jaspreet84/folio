"""Single-process timing of the fix-mode ladder A, plus size measures, on a sample and the slowest cases.

usage: python3 timing2.py results.jsonl TUNE... -- HELD...
"""
import hashlib
import json
import os
import random
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(1, os.path.join(HERE, '..', 'proto'))
import proto as P  # noqa: E402
import ladder as L  # noqa: E402
from corpus_extract import blocks_from  # noqa: E402

recs = [json.loads(l) for l in open(sys.argv[1], encoding='utf-8')]
args = sys.argv[2:]
cut = args.index('--')
blocks = {}
for b in blocks_from(args[:cut]) + blocks_from(args[cut + 1:]):
    blocks.setdefault(hashlib.sha1(b.encode('utf-8')).hexdigest()[:12], b)
flows = [r for r in recs if r['kind'] == 'flow' and r.get('outcome') == 'parsed']
random.seed(7)
sample = random.sample(flows, 1500)
slow = sorted(flows, key=lambda r: -(r['t_prep'] + r['t_fix']))[:30]
P.FIX = True


def measure(r):
    b = blocks[r['h']]
    title, kw, st = P.kind_of(b)
    t0 = time.perf_counter()
    g = P.parse_flow(st[0], st[1:])
    t1 = time.perf_counter()
    prep = P.flow_prepare(g)
    t2 = time.perf_counter()
    d = 'LR' if g.dir in ('LR', 'RL') else 'TD'
    profs = {p[0]: p for p in (L.FLOW_LR if d == 'LR' else L.FLOW_TD)}
    for name in L.LADDERS['A']:
        if name in profs:
            n_, lw, gap, chan, dr = profs[name]
            P.flow_draw(prep, P.Prof(n_, lw, gap, chan, dr))
    t3 = time.perf_counter()
    widest = max(len(v) for v in prep['order'].values()) if prep['order'] else 0
    elems = sum(len(v) for v in prep['order'].values())
    return {'parse': t1 - t0, 'prep': t2 - t1, 'draw': t3 - t2, 'total': t3 - t0, 'widest': widest,
            'elems': elems, 'n': r['n'], 'm': r['m'], 'dummies': r['dummies']}


def q(v, p):
    v = sorted(v)
    return v[min(len(v) - 1, int(p * len(v)))]


res = [measure(r) for r in sample]
tot = [x['total'] for x in res]
print('sample of %d flowcharts, fix mode, ladder A, single process:' % len(res))
print('  total p50 %.1f ms p90 %.1f p95 %.1f p99 %.1f max %.0f ms' % (
    q(tot, .5) * 1e3, q(tot, .9) * 1e3, q(tot, .95) * 1e3, q(tot, .99) * 1e3, max(tot) * 1e3))
for key in ('widest', 'elems', 'dummies'):
    v = [x[key] for x in res]
    print('  %s p50 %d p90 %d p99 %d max %d' % (key, q(v, .5), q(v, .9), q(v, .99), max(v)))
print('slowest 30 (from the parallel run), re-timed:')
for r in slow:
    x = measure(r)
    print('  total %6.0f ms (prep %5.0f, draw %6.0f)  nodes %3d edges %3d dummies %3d widest rank %3d elements %4d' % (
        x['total'] * 1e3, x['prep'] * 1e3, x['draw'] * 1e3, x['n'], x['m'], x['dummies'], x['widest'], x['elems']))
# how many parsed flowcharts exceed candidate caps (prepare only)
W, EL = [], []
for r in flows:
    title, kw, st = P.kind_of(blocks[r['h']])
    prep = P.flow_prepare(P.parse_flow(st[0], st[1:]))
    W.append(max(len(v) for v in prep['order'].values()) if prep['order'] else 0)
    EL.append(sum(len(v) for v in prep['order'].values()))
print('all %d parsed flowcharts: widest rank p50 %d p99 %d p99.9 %d max %d; elements p99 %d p99.9 %d max %d' % (
    len(W), q(W, .5), q(W, .99), q(W, .999), max(W), q(EL, .99), q(EL, .999), max(EL)))
for cap in (30, 40, 60):
    print('  widest rank > %d: %d' % (cap, sum(1 for w in W if w > cap)))
for cap in (200, 300, 400):
    print('  elements > %d: %d' % (cap, sum(1 for e in EL if e > cap)))
