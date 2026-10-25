import sys
sys.path.insert(0, sys.argv[1])
import proto
for f in sys.argv[2:]:
    src = open(f).read()
    o, rows, step, notes, kind = proto.render(src, 10**6, natural=True)
    nat = max(proto.width(r) for r in rows)
    out = [f.split('/')[-1], 'natural %dx%d' % (nat, len(rows))]
    for w in (38, 40, 78, 80, 100, 120):
        o, rows, step, notes, kind = proto.render(src, w)
        if o == 'drawn':
            out.append('%d:%s %dx%d' % (w, step, max(proto.width(r) for r in rows), len(rows)))
        else:
            out.append('%d:%s' % (w, o))
    print(' | '.join(out))
