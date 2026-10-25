"""Mock-ups with the fixes on (proto2, FIX=True)."""
import sys

import proto as P

P.FIX = True
for path, widths in (('flow1.mmd', (80, 38)), ('seq1.mmd', (80, 40, 38)), ('../review/share.mmd', (80,)),
                     ('../review/skip.mmd', (80,))):
    src = open(path, encoding='utf-8').read()
    for w in widths:
        rows, outcome, step, notes = P.framed(src, w)
        print('### %s at %d: %s %s %s' % (path, w, outcome, step, notes))
        print('\n'.join(rows))
