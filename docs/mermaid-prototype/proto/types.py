"""Count diagram types and a few flowchart features in the corpus sample."""
import re
import sys
from collections import Counter

sys.path.insert(0, sys.argv[1])
import proto  # noqa: E402
from corpus_extract import blocks_from  # noqa: E402

blocks = blocks_from(sys.argv[2:])
types = Counter()
feat = Counter()
nflow = 0
for b in blocks:
    try:
        title, kw, st = proto.kind_of(b)
    except proto.Unsupported:
        types['(empty or bad front matter)'] += 1
        continue
    k = kw.split(':')[0]
    if k in ('graph', 'flowchart', 'flowchart-v2', 'flowchart-elk'):
        k = 'flowchart'
    if k in ('stateDiagram', 'stateDiagram-v2'):
        k = 'state'
    types[k] += 1
    if k == 'flowchart':
        nflow += 1
        text = '\n'.join(st)
        if re.search(r'^\s*subgraph\b', text, re.M):
            feat['subgraph'] += 1
        if re.search(r'^\s*(style|classDef|class|linkStyle|click)\b', text, re.M):
            feat['style/classDef/class/linkStyle/click'] += 1
        hdr = st[0].split()
        d = hdr[1].rstrip(';') if len(hdr) > 1 else 'TB'
        feat['dir ' + d] += 1
total = sum(types.values())
print('blocks', len(blocks), 'typed', total)
for k, v in types.most_common(20):
    print('%-28s %6d  %5.1f%%' % (k, v, 100.0 * v / total))
print('--- flowchart features (of %d)' % nflow)
for k, v in feat.most_common():
    print('%-40s %6d  %5.1f%%' % (k, v, 100.0 * v / nflow))
