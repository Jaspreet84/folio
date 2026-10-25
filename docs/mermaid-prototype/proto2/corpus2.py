"""Run the checked prototype over the tuning sample and a held-out sample.

usage: python3 corpus2.py OUT.jsonl TUNE_FILE... -- HELD_FILE...
Blocks are read as data only. Held-out blocks exclude every block in the tuning sample.
"""
import hashlib
import json
import os
import sys
import time
import unicodedata
from multiprocessing import Pool

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(1, os.path.join(HERE, '..', 'proto'))
import proto as P  # noqa: E402  (proto2/proto.py)
import ladder as L  # noqa: E402
from corpus_extract import blocks_from  # noqa: E402

assert os.path.dirname(os.path.abspath(P.__file__)) == HERE


def has_cjk(s):
    return any(unicodedata.east_asian_width(c) in 'WF' and unicodedata.category(c).startswith('L') for c in s)


def work(item):
    which, b = item
    rec = {'set': which, 'h': hashlib.sha1(b.encode('utf-8')).hexdigest()[:12]}
    try:
        title, kw, st = P.kind_of(b)
    except P.Unsupported as e:
        rec.update(kind='none', why=str(e))
        return rec
    except Exception as e:
        rec.update(kind='none', why='crash ' + type(e).__name__)
        return rec
    rec['kw'] = kw
    if kw in ('flowchart', 'graph', 'flowchart-v2', 'flowchart-elk'):
        rec['kind'] = 'flow'
    elif kw == 'sequenceDiagram':
        rec['kind'] = 'seq'
    else:
        rec['kind'] = 'other'
        return rec
    cjk = has_cjk(b)
    rec['cjk'] = cjk
    try:
        if rec['kind'] == 'flow':
            g = P.parse_flow(st[0], st[1:])
            nn = len([n for n in g.nodes if not n.get('dead')])
            vis_edges = [e for e in g.edges if e['stroke'] != 'invisible']
            rec.update(dir=g.dir, n=nn, m=len(vis_edges), sub='subgraph' in g.simplified,
                       lrself=g.dir in ('LR', 'RL') and any(e['a'] == e['b'] for e in vis_edges))
            if nn > 150 or len(g.edges) > 300 or len(b) > 65536:
                rec['outcome'] = 'cap'
                return rec
            t0 = time.perf_counter()
            prep = P.flow_prepare(g)
            t1 = time.perf_counter()
            rec['t_prep'] = t1 - t0
            rec['dummies'] = sum(len(ch) - 2 for ch in prep['chains'].values())
            rec['span'] = sum(abs(prep['rank'][e['a']] - prep['rank'][e['b']])
                              for e in vis_edges if e['a'] in prep['rank'] and e['b'] in prep['rank'])
            rec['ranks'] = prep['R'] + 1
            steps = {}
            steps['orig'] = L.flow_steps(g, prep, False)
            t2 = time.perf_counter()
            steps['fix'] = L.flow_steps(g, prep, True)
            t3 = time.perf_counter()
            rec['t_orig'] = t2 - t1
            rec['t_fix'] = t3 - t2
            if cjk:
                steps['cjkfix'] = L.flow_steps(g, prep, True, cjk=True)
            rec['steps'] = steps
            rec['notes'] = sorted(g.simplified)
        else:
            sd = P.parse_seq(st[0], st[1:])
            rec.update(n=len(sd.parts), ev=len(sd.events))
            if len(sd.parts) > 40 or len(sd.events) > 500:
                rec['outcome'] = 'cap'
                return rec
            rec['skip'] = any(ev[0] == 'msg' and abs(ev[1] - ev[2]) > 1 for ev in sd.events)
            t1 = time.perf_counter()
            steps = {'orig': L.seq_steps(sd, False)}
            t2 = time.perf_counter()
            steps['fix'] = L.seq_steps(sd, True)
            rec['t_orig'] = t2 - t1
            rec['t_fix'] = time.perf_counter() - t2
            if cjk:
                steps['cjkfix'] = L.seq_steps(sd, True, cjk=True)
            rec['steps'] = steps
            rec['notes'] = sorted(sd.simplified)
    except P.Unsupported as e:
        rec['outcome'] = 'source'
        rec['why'] = str(e)[:80]
        return rec
    except Exception as e:
        rec['outcome'] = 'crash'
        rec['why'] = (type(e).__name__ + ' ' + str(e))[:80]
        return rec
    rec['outcome'] = 'parsed'
    return rec


def main():
    out = sys.argv[1]
    args = sys.argv[2:]
    cut = args.index('--')
    tune = blocks_from(args[:cut])
    seen = set(tune)
    held = [b for b in blocks_from(args[cut + 1:]) if b not in seen]
    print('tuning blocks', len(tune), 'held-out blocks', len(held), flush=True)
    items = [('tune', b) for b in tune] + [('held', b) for b in held]
    t0 = time.time()
    with Pool(8, maxtasksperchild=500) as pool, open(out, 'w', encoding='utf-8') as f:
        for i, rec in enumerate(pool.imap_unordered(work, items, chunksize=8)):
            f.write(json.dumps(rec) + '\n')
            if i % 2000 == 0:
                print(i, '%.0fs' % (time.time() - t0), flush=True)
    print('done %.0fs' % (time.time() - t0))


if __name__ == '__main__':
    main()
