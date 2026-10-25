"""Section 7.3 table under the strict definition (fix mode, ladder A, plus ladders B and C for LR)."""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import proto as P  # noqa: E402
import ladder as L  # noqa: E402

for f in sys.argv[1:]:
    src = open(f, encoding='utf-8').read()
    try:
        title, kw, st = P.kind_of(src)
    except P.Unsupported as e:
        print(os.path.basename(f), 'source:', e)
        continue
    try:
        if kw == 'sequenceDiagram':
            sd = P.parse_seq(st[0], st[1:])
            steps = L.seq_steps(sd, True)
            ladders = {'A': ['N', 'C', 'L16', 'L8']}
            ok = lambda m: L.seq_ok(m, True, True)
        elif kw in ('flowchart', 'graph'):
            g = P.parse_flow(st[0], st[1:])
            prep = P.flow_prepare(g)
            steps = L.flow_steps(g, prep, True)
            ladders = {k: [s for s in v if s in steps] for k, v in L.LADDERS.items()}
            ok = lambda m: L.flow_ok(m, True)
        else:
            print(os.path.basename(f), 'source: type', kw)
            continue
    except P.Unsupported as e:
        print(os.path.basename(f), 'source:', e)
        continue
    nat = steps['N']
    out = [os.path.basename(f), 'natural %dx%d checks %s' % (nat[0], nat[1], nat[2:])]
    for lname, lad in ladders.items():
        if lname != 'A' and lad == ladders['A']:
            continue
        cells = []
        for w in (38, 40, 51, 78, 100, 118):
            ch = L.choose(steps, lad, w, ok)
            cells.append('%d:%s' % (w, ch if ch == 'outline' else '%s %dx%d' % (ch, steps[ch][0], steps[ch][1])))
        out.append('[%s] ' % lname + ' | '.join(cells))
    print('  '.join(out))
