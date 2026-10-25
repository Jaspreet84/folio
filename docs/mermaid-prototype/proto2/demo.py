"""Render small inputs in original and fix mode and print the checks."""
import sys

import proto as P
import ladder as L

for path in sys.argv[1:]:
    src = open(path, encoding='utf-8').read()
    title, kw, st = P.kind_of(src)
    for fix in (False, True):
        if kw == 'sequenceDiagram':
            sd = P.parse_seq(st[0], st[1:])
            steps, rows = L.seq_steps(sd, fix, want_rows=True)
            name = 'N'
            print('=== %s %s seq %s  [w h viol badmsg framej crossings bad] = %s' % (
                path.split('/')[-1], 'fix' if fix else 'orig', name, steps[name]))
        else:
            g = P.parse_flow(st[0], st[1:])
            prep = P.flow_prepare(g)
            steps, rows = L.flow_steps(g, prep, fix, want_rows=True)
            name = 'N'
            print('=== %s %s flow %s  [w h viol falsepairs crossings bad hard dropped] = %s' % (
                path.split('/')[-1], 'fix' if fix else 'orig', name, steps[name]))
        print('\n'.join(rows[name]))
