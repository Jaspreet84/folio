"""Record what the prototype's parsers make of every corpus block, for the
M1 differential test (`examples/mermaid_parse_diff.rs`).

usage: python3 -I m1_dump.py OUT.jsonl TUNE_FILE... -- HELD_FILE...

Writes one JSON line per block: the block source, its sample, and either the
reason it was not read or the parsed flowchart or sequence diagram, with its
outline or transcript at widths 40 and 78 and unwrapped. Blocks are read as
data only. Held-out blocks exclude every block in the tuning sample.
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(1, os.path.join(HERE, '..', 'proto'))
import proto as P  # noqa: E402  (proto2/proto.py)
from corpus_extract import blocks_from  # noqa: E402

assert os.path.dirname(os.path.abspath(P.__file__)) == HERE
P.FIX = True
P.CJK_BREAK = True

FLOW = ('flowchart', 'graph', 'flowchart-v2', 'flowchart-elk')
WIDE = 10 ** 6


def clean(x):
    """Replace lone surrogates (from entities such as #55357;) so the line is
    valid UTF-8."""
    if isinstance(x, str):
        return ''.join('�' if 0xD800 <= ord(c) <= 0xDFFF else c for c in x)
    if isinstance(x, (list, tuple)):
        return [clean(v) for v in x]
    if isinstance(x, dict):
        return {k: clean(v) for k, v in x.items()}
    return x


def flow_record(g):
    nodes = [{'id': n['id'], 'label': n['label'], 'shape': n['shape'],
              'slant': list(n['slant']) if n['slant'] else None,
              'explicit': n['explicit'], 'dead': bool(n.get('dead')),
              'cluster': n['cluster']} for n in g.nodes]
    edges = [{'a': e['a'], 'b': e['b'], 'stroke': e['stroke'], 'ha': e['ha'], 'hb': e['hb'],
              'label': e['label'], 'minlen': e['minlen']} for e in g.edges]
    clusters = [{'id': cid, 'title': c['title'], 'parent': c['parent'], 'members': c['members']}
                for cid, c in g.clusters.items()]
    return {'dir': g.dir, 'nodes': nodes, 'edges': edges, 'clusters': clusters,
            'simplified': sorted(g.simplified)}


def seq_record(sd):
    parts = [{'id': p['id'], 'label': p['label'], 'kind': p['kind']} for p in sd.parts]
    return {'parts': parts, 'events': [list(ev) for ev in sd.events],
            'autonumber': list(sd.autonumber) if sd.autonumber else None,
            'simplified': sorted(sd.simplified)}


def work(which, src):
    rec = {'set': which, 'src': src}
    try:
        title, kw, st = P.kind_of(src)
    except P.Unsupported as e:
        rec.update(kind='none', why=str(e))
        return rec
    except Exception as e:
        rec.update(kind='none', crash=type(e).__name__ + ' ' + str(e)[:80])
        return rec
    rec.update(title=title, kw=kw)
    if kw in FLOW:
        rec['kind'] = 'flow'
    elif kw == 'sequenceDiagram':
        rec['kind'] = 'seq'
    else:
        rec['kind'] = 'other'
        return rec
    try:
        if rec['kind'] == 'flow':
            g = P.parse_flow(st[0], st[1:])
            rec['g'] = flow_record(g)
            rec['outline'] = {str(w): P.flow_outline(g, w) for w in (40, 78, WIDE)}
        else:
            sd = P.parse_seq(st[0], st[1:])
            rec['sd'] = seq_record(sd)
            rec['outline'] = {str(w): P.seq_transcript(sd, w) for w in (40, 78, WIDE)}
    except P.Unsupported as e:
        rec['why'] = str(e)
    except Exception as e:
        rec['crash'] = type(e).__name__ + ' ' + str(e)[:80]
    return rec


def main():
    out = sys.argv[1]
    args = sys.argv[2:]
    cut = args.index('--')
    tune = blocks_from(args[:cut])
    seen = set(tune)
    held = [b for b in blocks_from(args[cut + 1:]) if b not in seen]
    print('tuning blocks', len(tune), 'held-out blocks', len(held), flush=True)
    with open(out, 'w', encoding='utf-8') as f:
        for which, blocks in (('tune', tune), ('held', held)):
            for b in blocks:
                f.write(json.dumps(clean(work(which, b)), ensure_ascii=False) + '\n')
    print('done')


if __name__ == '__main__':
    main()
