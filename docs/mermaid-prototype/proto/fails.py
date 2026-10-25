"""Print the statements that the prototype parser rejects, grouped roughly."""
import re
import sys
from collections import Counter

sys.path.insert(0, sys.argv[1])
import proto  # noqa: E402
from corpus_extract import blocks_from  # noqa: E402

want = sys.argv[2]
blocks = blocks_from(sys.argv[3:])
shown = 0
pat = Counter()
for b in blocks:
    try:
        title, kw, st = proto.kind_of(b)
    except proto.Unsupported:
        continue
    kind = 'flowchart' if kw in ('flowchart', 'graph') else ('sequence' if kw == 'sequenceDiagram' else None)
    if kind != want:
        continue
    for s in st[1:]:
        try:
            if kind == 'flowchart':
                proto.parse_flow(st[0], [s])
            else:
                proto.parse_seq(st[0], [s])
        except proto.Unsupported as e:
            msg = str(e)
            if 'subgraph' in msg or 'end' in msg or 'block' in msg or 'outside' in msg or 'no nodes' in msg:
                continue
            key = re.sub(r'[A-Za-z0-9_]+', 'w', s)[:30]
            pat[key] += 1
            if shown < 60:
                print(repr(s[:90]), '->', msg[:50])
                shown += 1
            break
print('--- shapes')
for k, v in pat.most_common(30):
    print(v, repr(k))
