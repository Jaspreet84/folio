"""Dump flowcharts and the prototype's drawings for the Rust differential test (M2).

usage: python3 -I m2_dump.py OUT.jsonl [--tune-limit N] TUNE_FILE... -- HELD_FILE...

For every flowchart block that parse_flow accepts, that has no subgraphs and
is within the caps (150 nodes, 300 edges), one JSON line holds the graph in
the Rust IR's serde shape and, for every ladder step, the prototype's rows
and metrics, as ladder.flow_steps(g, prep, True, cjk=True, want_rows=True)
computes them. A step or preparation that crashes is recorded as a crash.
Blocks are read as data only. Held-out blocks exclude every block in the
tuning sample. --tune-limit keeps a fixed random sample of tuning blocks.

The Rust side is examples/mermaid_draw_diff.rs.
"""
import hashlib
import json
import os
import random
import sys
import time
from multiprocessing import Pool

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(1, os.path.join(HERE, '..', 'proto'))
import proto as P  # noqa: E402  (proto2/proto.py)
import check as C  # noqa: E402
import ladder as L  # noqa: E402
from corpus_extract import blocks_from  # noqa: E402

assert os.path.dirname(os.path.abspath(P.__file__)) == HERE

SHAPE = {'rect': 'Rect', 'round': 'Round', 'stadium': 'Stadium', 'subroutine': 'Subroutine',
         'cylinder': 'Cylinder', 'circle': 'Circle', 'dcircle': 'DCircle', 'decision': 'Decision',
         'hexagon': 'Hexagon', 'slant': 'Slant', 'asym': 'Asym', 'bare': 'Bare'}
STROKE = {'solid': 'Solid', 'dotted': 'Dotted', 'thick': 'Thick', 'invisible': 'Invisible'}
HEADS = {'none': 'None', 'arrow': 'Arrow', 'circle': 'Circle', 'cross': 'Cross'}


def to_ir(g):
    nodes = [{'id': n['id'], 'label': n['label'], 'shape': SHAPE[n['shape']], 'cluster': None,
              'slant': list(n['slant']) if n['slant'] else None, 'explicit': n['explicit'],
              'dead': bool(n.get('dead'))} for n in g.nodes]
    edges = [{'from': e['a'], 'to': e['b'], 'stroke': STROKE[e['stroke']], 'head_from': HEADS[e['ha']],
              'head_to': HEADS[e['hb']], 'label': e['label'], 'min_len': e['minlen']} for e in g.edges]
    return {'dir': g.dir, 'nodes': nodes, 'edges': edges, 'clusters': [], 'simplified': sorted(g.simplified)}


def label_widths(g):
    """The prototype's width of every label, for spotting width-table differences."""
    out = [P.width(n['label'].replace('\n', ' ')) for n in g.nodes]
    out += [P.width((e['label'] or '').replace('\n', ' ')) for e in g.edges]
    return out


def label_graphemes(g):
    """The width of each of the prototype's graphemes in every label, for spotting
    segmentation differences (it joins only zero-width characters to the one before)."""
    labels = [n['label'] for n in g.nodes] + [e['label'] or '' for e in g.edges]
    return [[P.width(x) for x in P.graphemes(l.replace('\n', ' '))] for l in labels]


def work(item):
    which, b = item
    rec = {'set': which, 'h': hashlib.sha1(b.encode('utf-8')).hexdigest()[:12]}
    try:
        title, kw, st = P.kind_of(b)
        if kw not in ('flowchart', 'graph', 'flowchart-v2', 'flowchart-elk'):
            return None
        g = P.parse_flow(st[0], st[1:])
    except Exception:
        return None
    if g.clusters or len(b) > 65536:
        return None
    nn = len([n for n in g.nodes if not n.get('dead')])
    if nn > 150 or len(g.edges) > 300:
        return None
    rec['graph'] = to_ir(g)
    rec['pw'] = label_widths(g)
    rec['pg'] = label_graphemes(g)
    P.FIX, P.CJK_BREAK = True, True
    t0 = time.perf_counter()
    try:
        prep = P.flow_prepare(g)
    except Exception as e:
        rec['prep_crash'] = (type(e).__name__ + ' ' + str(e))[:100]
        return rec
    rec['widest'] = max(len(v) for v in prep['order'].values()) if prep['order'] else 0
    rec['elems'] = sum(len(v) for v in prep['order'].values())
    d = 'LR' if g.dir in ('LR', 'RL') else 'TD'
    edges = {k: (e['a'], e['b']) for k, e in enumerate(g.edges) if e['stroke'] != 'invisible'}
    steps = {}
    for name, lw, gap, chan, dr in (L.FLOW_LR if d == 'LR' else L.FLOW_TD):
        try:
            rows, viol = P.flow_draw(prep, P.Prof(name, lw, gap, chan, dr))
            cv = P.LAST['cv']
            crossings, bad = C.cell_check(cv)
            falsep = C.flow_check(cv, edges)
            m = [max((P.width(r) for r in rows), default=0), len(rows), viol, falsep, crossings, bad,
                 cv.hard_cycles, cv.dropped]
            steps[name] = {'rows': rows, 'm': m}
        except Exception as e:
            steps[name] = {'crash': (type(e).__name__ + ' ' + str(e))[:100]}
    rec['t'] = time.perf_counter() - t0
    rec['steps'] = steps
    return rec


def main():
    args = sys.argv[1:]
    out = args.pop(0)
    limit = None
    if args and args[0] == '--tune-limit':
        limit = int(args[1])
        args = args[2:]
    cut = args.index('--')
    tune = blocks_from(args[:cut])
    seen = set(tune)
    held = [b for b in blocks_from(args[cut + 1:]) if b not in seen]
    if limit is not None and limit < len(tune):
        random.seed(2)
        tune = random.sample(tune, limit)
    print('tuning blocks', len(tune), 'held-out blocks', len(held), flush=True)
    items = [('tune', b) for b in tune] + [('held', b) for b in held]
    t0 = time.time()
    n = 0
    with Pool(8, maxtasksperchild=200) as pool, open(out, 'w', encoding='utf-8') as f:
        for i, rec in enumerate(pool.imap_unordered(work, items, chunksize=4)):
            if rec is not None:
                f.write(json.dumps(rec, ensure_ascii=False) + '\n')
                n += 1
            if i % 2000 == 0:
                print(i, n, '%.0fs' % (time.time() - t0), flush=True)
    print('done: %d flowcharts in %.0fs' % (n, time.time() - t0))


if __name__ == '__main__':
    main()
