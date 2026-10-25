"""Tables for the revised plan from results.jsonl."""
import json
import sys
from collections import Counter, defaultdict

sys.path.insert(0, __file__.rsplit('/', 1)[0])
import ladder as L  # noqa: E402

WIDTHS = [38, 51, 78, 100, 118, 158]
recs = [json.loads(l) for l in open(sys.argv[1], encoding='utf-8')]


def pct(a, b):
    return '%.1f%%' % (100.0 * a / b) if b else '-'


def q(v, p):
    v = sorted(v)
    return v[min(len(v) - 1, int(p * len(v)))] if v else 0


for S in ('tune', 'held'):
    R = [r for r in recs if r['set'] == S]
    print('\n' + '=' * 30, S, 'blocks', len(R))
    kinds = Counter(r['kind'] for r in R)
    print('kinds', dict(kinds))
    for K in ('flow', 'seq'):
        rr = [r for r in R if r['kind'] == K]
        oc = Counter(r.get('outcome') for r in rr)
        print('\n--', K, 'total', len(rr), dict(oc))
        why = Counter((r.get('why') or '')[:50] for r in rr if r.get('outcome') == 'source')
        print('   top source reasons:', why.most_common(8))
        crashes = Counter((r.get('why') or '')[:60] for r in rr if r.get('outcome') == 'crash')
        if crashes:
            print('   crashes:', crashes.most_common(5))
        P = [r for r in rr if r.get('outcome') == 'parsed']
        if K == 'flow':
            kw = Counter(r.get('kw') for r in rr)
            print('   keywords', dict(kw))
            dirs = Counter(r['dir'] for r in P)
            print('   dirs', dict(dirs), 'subgraph', sum(r['sub'] for r in P), 'LR self-loop', sum(r['lrself'] for r in P),
                  'n==0', sum(1 for r in P if r['n'] == 0), 'n==1', sum(1 for r in P if r['n'] == 1),
                  'm==0', sum(1 for r in P if r['m'] == 0))
            nosub = [r for r in P if not r['sub']]
            sub = [r for r in P if r['sub']]

            def lad(r, name):
                return [s for s in L.LADDERS[name] if s in r['steps']['fix']]

            def drawn(group, mode, strict, ladder='A', W=78):
                c = 0
                for r in group:
                    st = r['steps'][mode]
                    names = [s for s in L.LADDERS[ladder] if s in st]
                    if L.choose(st, names, W, lambda m: L.flow_ok(m, strict)) != 'outline':
                        c += 1
                return c
            print('   %-58s %s' % ('drawn share at width', '  '.join('%7d' % w for w in WIDTHS)))
            rows = [
                ('fits only, orig (old definition), all parsed', P, 'orig', False, 'A'),
                ('fits only, orig, no subgraphs', nosub, 'orig', False, 'A'),
                ('strict, orig, no subgraphs', nosub, 'orig', True, 'A'),
                ('strict, fix, no subgraphs [ladder A]', nosub, 'fix', True, 'A'),
                ('strict, fix, no subgraphs [ladder B]', nosub, 'fix', True, 'B'),
                ('strict, fix, no subgraphs [ladder C]', nosub, 'fix', True, 'C'),
                ('strict, fix, subgraph subset, frames NOT drawn', sub, 'fix', True, 'A'),
            ]
            for label, grp, mode, strict, ladder in rows:
                print('   %-58s %s   (n=%d)' % (label, '  '.join('%7s' % pct(drawn(grp, mode, strict, ladder, w), len(grp))
                                                                    for w in WIDTHS), len(grp)))
            print('   %-58s %s' % ('strict, fix, all parsed, subgraphs -> outline (M2 policy)',
                                   '  '.join('%7s' % pct(drawn(nosub, 'fix', True, 'A', w), len(P)) for w in WIDTHS)))
            # what went wrong in the old definition at 78
            for mode in ('orig', 'fix'):
                bad = Counter()
                for r in nosub:
                    st = r['steps'][mode]
                    names = [s for s in L.LADDERS['A'] if s in st]
                    ch = L.choose(st, names, 78, lambda m: True)
                    if ch == 'outline':
                        continue
                    m = st[ch]
                    bad['any'] += any([m[2], m[3], m[5], m[6], m[7]])
                    bad['text over line'] += m[2] > 0
                    bad['false connection'] += m[3] > 0
                    bad['crossing cells'] += m[4] > 0
                    bad['bad overlap'] += m[5] > 0
                    bad['hard-constraint conflict'] += m[6] > 0
                    bad['dropped LR self-loop'] += m[7] > 0
                nd = drawn(nosub, mode, False, 'A', 78)
                print('   at 78, %s mode, of %d fitting drawings (no subgraphs): %s' % (
                    mode, nd, ', '.join('%s %d (%s)' % (k, v, pct(v, nd)) for k, v in bad.items())))
            # steps used at 78, strict fix, ladder A
            for ladder in ('A', 'B', 'C'):
                used = Counter()
                for r in nosub:
                    st = r['steps']['fix']
                    names = [s for s in L.LADDERS[ladder] if s in st]
                    used[L.choose(st, names, 78, lambda m: L.flow_ok(m, True))] += 1
                print('   steps used at 78 (strict fix, ladder %s): %s' % (ladder, dict(used)))
            # LR direction kept and heights
            lr = [r for r in nosub if r['dir'] in ('LR', 'RL')]
            for W in (51, 78, 100):
                parts = []
                for ladder in ('A', 'B', 'C'):
                    keep = turned = 0
                    hts = []
                    for r in lr:
                        st = r['steps']['fix']
                        names = [s for s in L.LADDERS[ladder] if s in st]
                        ch = L.choose(st, names, W, lambda m: L.flow_ok(m, True))
                        if ch == 'outline':
                            continue
                        if ch in ('N', 'C', 'L16lr', 'L8lr'):
                            keep += 1
                        else:
                            turned += 1
                        hts.append(st[ch][1] / max(1, st['N'][1]))
                    parts.append('%s: kept LR %d, turned %d, drawn %s, median height x%.2f' % (
                        ladder, keep, turned, pct(keep + turned, len(lr)), q(hts, .5)))
                print('   LR (%d) at %d -> %s' % (len(lr), W, ' | '.join(parts)))
            # CJK
            cj = [r for r in nosub if r.get('cjk')]
            if cj:
                print('   CJK subset n=%d: fix %s ; fix + breaks between wide chars %s' % (
                    len(cj),
                    ' '.join('%d:%s' % (w, pct(drawn(cj, 'fix', True, 'A', w), len(cj))) for w in (38, 51, 78)),
                    ' '.join('%d:%s' % (w, pct(drawn(cj, 'cjkfix', True, 'A', w), len(cj))) for w in (38, 51, 78))))
            # time and size
            t = [r['t_prep'] + r['t_fix'] for r in P]
            print('   python time per flowchart, fix mode, all steps (%d to %d steps): p50 %.1f ms p95 %.1f p99 %.1f max %.0f ms' % (
                4, 8, q(t, .5) * 1e3, q(t, .95) * 1e3, q(t, .99) * 1e3, max(t) * 1e3))
            dm = [r['dummies'] for r in P]
            print('   dummies per flowchart: p50 %d p90 %d p99 %d max %d' % (q(dm, .5), q(dm, .9), q(dm, .99), max(dm)))
            buckets = [(0, 50), (50, 100), (100, 200), (200, 400), (400, 10 ** 9)]
            for lo, hi in buckets:
                g = [r for r in P if lo <= r['dummies'] < hi]
                if g:
                    tt = [r['t_prep'] + r['t_fix'] for r in g]
                    print('     dummies %d-%s: n=%d time p50 %.0f ms p99 %.0f ms max %.0f ms' % (
                        lo, hi if hi < 10 ** 9 else 'inf', len(g), q(tt, .5) * 1e3, q(tt, .99) * 1e3, max(tt) * 1e3))
            slow = sorted(P, key=lambda r: -(r['t_prep'] + r['t_fix']))[:5]
            for r in slow:
                print('     slow: %.0f ms nodes %d edges %d dummies %d ranks %d' % (
                    (r['t_prep'] + r['t_fix']) * 1e3, r['n'], r['m'], r['dummies'], r['ranks']))
        else:
            def drawn(group, mode, strict, W, frames=False):
                c = 0
                for r in group:
                    st = r['steps'][mode]
                    if L.choose(st, ['N', 'C', 'L16', 'L8'], W, lambda m: L.seq_ok(m, strict, frames)) != 'outline':
                        c += 1
                return c
            print('   %-58s %s' % ('drawn share at width', '  '.join('%7d' % w for w in WIDTHS)))
            for label, mode, strict, frames in [('fits only, orig (old definition)', 'orig', False, False),
                                                ('strict, orig (messages only)', 'orig', True, False),
                                                ('strict, orig (messages and frames)', 'orig', True, True),
                                                ('strict, fix', 'fix', True, True)]:
                print('   %-58s %s   (n=%d)' % (label, '  '.join('%7s' % pct(drawn(P, mode, strict, w, frames), len(P))
                                                                    for w in WIDTHS), len(P)))
            print('   diagrams with a message that skips a lifeline:', sum(r['skip'] for r in P), pct(sum(r['skip'] for r in P), len(P)))
            for mode in ('orig', 'fix'):
                bad = Counter()
                for r in P:
                    st = r['steps'][mode]
                    ch = L.choose(st, ['N', 'C', 'L16', 'L8'], 78, lambda m: True)
                    if ch == 'outline':
                        continue
                    m = st[ch]
                    bad['text over line'] += m[2] > 0
                    bad['message junction'] += m[3] > 0
                    bad['frame junction'] += m[4] > 0
                    bad['bad overlap'] += m[6] > 0
                nd = drawn(P, mode, False, 78)
                print('   at 78, %s mode, of %d fitting drawings: %s' % (
                    mode, nd, ', '.join('%s %d (%s)' % (k, v, pct(v, nd)) for k, v in bad.items())))
            cj = [r for r in P if r.get('cjk')]
            if cj:
                print('   CJK subset n=%d: fix %s ; with breaks %s' % (
                    len(cj), ' '.join('%d:%s' % (w, pct(drawn(cj, 'fix', True, w, True), len(cj))) for w in (38, 51, 78)),
                    ' '.join('%d:%s' % (w, pct(drawn(cj, 'cjkfix', True, w, True), len(cj))) for w in (38, 51, 78))))
            t = [r['t_fix'] for r in P]
            print('   python time per sequence diagram, fix mode, 4 steps: p50 %.1f ms p99 %.1f max %.0f ms' % (
                q(t, .5) * 1e3, q(t, .99) * 1e3, max(t) * 1e3))
