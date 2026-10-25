"""Run the prototype over the Sourcegraph sample (read as data only)."""
import json
import os
import re
import sys
import time
from collections import Counter

sys.path.insert(0, sys.argv[1])
import proto  # noqa: E402

TARGETS = [38, 78, 100, 118, 158]


def blocks_from(paths):
    seen, out = set(), []
    for p in paths:
        ev = ''
        with open(p, encoding='utf-8', errors='replace') as f:
            for line in f:
                line = line.rstrip('\n')
                if line.startswith('event: '):
                    ev = line[7:]
                    continue
                if not line.startswith('data: ') or ev != 'matches':
                    continue
                try:
                    arr = json.loads(line[6:])
                except ValueError:
                    continue
                if not isinstance(arr, list):
                    continue
                for m in arr:
                    contents = [cm.get('content', '') for cm in m.get('chunkMatches') or []]
                    lms = m.get('lineMatches')
                    if lms:
                        v = sorted(((lm.get('lineNumber', 0), lm.get('line', '')) for lm in lms))
                        s, prev = '', None
                        for ln, l in v:
                            if prev is not None:
                                s += '\n\x00\n' if ln != prev + 1 else '\n'
                            s += l
                            prev = ln
                        contents.append(s)
                    for c in contents:
                        rest = c
                        while True:
                            i = rest.find('```mermaid')
                            if i < 0:
                                break
                            after = rest[i + 10:]
                            nl = after.find('\n')
                            if nl < 0:
                                break
                            if after[:nl].strip():
                                rest = after[nl:]
                                continue
                            body = after[nl + 1:]
                            end = body.find('\n```')
                            if end < 0:
                                end = body.find('```')
                            if end < 0:
                                break
                            b = body[:end]
                            rest = body[end + 3:]
                            if '\x00' in b:
                                continue
                            ls = [l for l in b.split('\n') if l.strip()]
                            ind = min((len(l) - len(l.lstrip()) for l in ls), default=0)
                            b = '\n'.join(l[ind:] if len(l) >= ind else l.lstrip() for l in b.split('\n'))
                            if b not in seen:
                                seen.add(b)
                                out.append(b)
    return out


def main():
    blocks = blocks_from(sys.argv[2:])
    stats = Counter()
    reasons = Counter()
    simpl = Counter()
    times = []
    widths = {'flowchart': [], 'sequence': []}
    for b in blocks:
        try:
            title, kw, st = proto.kind_of(b)
        except proto.Unsupported:
            continue
        if kw in ('flowchart', 'graph', 'flowchart-v2', 'flowchart-elk'):
            kind = 'flowchart'
        elif kw == 'sequenceDiagram':
            kind = 'sequence'
        else:
            continue
        if os.environ.get('KIND') and os.environ['KIND'] != kind:
            continue
        stats[kind + ' total'] += 1
        t0 = time.perf_counter()
        try:
            if kind == 'flowchart':
                g = proto.parse_flow(st[0], st[1:])
                nn = len([n for n in g.nodes if not n.get('dead')])
                if nn > 150 or len(g.edges) > 300 or len(b) > 65536:
                    stats[kind + ' over size cap'] += 1
                    continue
                prep = proto.flow_prepare(g)
                d = 'LR' if g.dir in ('LR', 'RL') else 'TD'
                ladder = [proto.Prof('normal', 28, 3, 4, d), proto.Prof('compact', 28, 1, 2, d)]
                if d == 'LR':
                    ladder += [proto.Prof('turned', 28, 3, 4, 'TD'), proto.Prof('turned compact', 28, 1, 2, 'TD')]
                ladder += [proto.Prof('labels 16', 16, 1, 2, 'TD'), proto.Prof('labels 8', 8, 1, 2, 'TD')]
                flags = g.simplified
                results = []
                for prof in ladder:
                    rows, viol = proto.flow_draw(prep, prof)
                    results.append((prof.name, max((proto.width(r) for r in rows), default=0), len(rows), viol))
            else:
                sd = proto.parse_seq(st[0], st[1:])
                if len(sd.parts) > 40 or len(sd.events) > 500:
                    stats[kind + ' over size cap'] += 1
                    continue
                flags = sd.simplified
                results = []
                for name, lw, gap, spacer in [('normal', 28, 3, True), ('compact', 28, 1, False),
                                              ('labels 16', 16, 1, False), ('labels 8', 8, 1, False)]:
                    rows, viol = proto.seq_draw(sd, lw, gap, spacer, name in ('normal', 'compact'))
                    results.append((name, max((proto.width(r) for r in rows), default=0), len(rows), viol))
        except proto.Unsupported as e:
            stats[kind + ' source'] += 1
            r = str(e)
            r = re.sub(r'"[^"]*"', '"…"', r)
            reasons[kind + ': ' + r] += 1
            continue
        except Exception as e:  # prototype bug
            stats[kind + ' crash'] += 1
            open('crash_%d.mmd' % stats[kind + ' crash'], 'w').write(b)
            reasons[kind + ' CRASH: ' + type(e).__name__ + ' ' + str(e)[:40]] += 1
            continue
        times.append(time.perf_counter() - t0)
        stats[kind + ' parsed'] += 1
        sub = 'subgraph' in flags
        if sub:
            stats[kind + ' parsed with subgraphs (frames not prototyped)'] += 1
        for f in flags:
            simpl[kind + ': ' + f] += 1
        if results[0][3]:
            stats[kind + ' natural render has text-over-line cells'] += 1
        natw, nath = results[0][1], results[0][2]
        widths[kind].append((natw, nath, sub))
        for T in TARGETS:
            step = next((name for name, w, h, v in results if w <= T), 'outline')
            stats['%s @%d %s' % (kind, T, step)] += 1
            if not sub:
                stats['%s @%d (no subgraph) %s' % (kind, T, step)] += 1
    print('unique blocks', len(blocks))
    for k in sorted(stats):
        print('%-60s %d' % (k, stats[k]))
    print('--- source fallback reasons (top 25)')
    for k, v in reasons.most_common(25):
        print('%6d  %s' % (v, k))
    print('--- simplified notes')
    for k, v in simpl.most_common():
        print('%6d  %s' % (v, k))
    for kind, ws in widths.items():
        if not ws:
            continue
        ws_sorted = sorted(w for w, h, s in ws)
        hs = sorted(h for w, h, s in ws)
        n = len(ws_sorted)
        q = lambda a, p: a[min(len(a) - 1, int(p * len(a)))]
        print('%s natural width p25=%d p50=%d p75=%d p90=%d; height p50=%d p90=%d; >48 rows: %d of %d' % (
            kind, q(ws_sorted, .25), q(ws_sorted, .5), q(ws_sorted, .75), q(ws_sorted, .9),
            q(hs, .5), q(hs, .9), sum(1 for h in hs if h > 48), n))
    times.sort()
    if times:
        print('time per diagram (all ladder steps) p50=%.1fms p95=%.1fms p99=%.1fms max=%.0fms (Python)' % (
            times[len(times) // 2] * 1e3, times[int(len(times) * .95)] * 1e3, times[int(len(times) * .99)] * 1e3,
            times[-1] * 1e3))


main()
