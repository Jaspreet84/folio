"""Count ```mermaid fences per file in Sourcegraph stream results (read as data only)."""
import json
import sys
from collections import Counter, defaultdict

per = defaultdict(set)
for p in sys.argv[1:]:
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
                key = (m.get('repository'), m.get('path'))
                for lm in m.get('lineMatches') or []:
                    if lm.get('line', '').strip().startswith('```mermaid'):
                        per[key].add(lm.get('lineNumber'))
                for cm in m.get('chunkMatches') or []:
                    start = cm.get('contentStart', {}).get('line', 0)
                    for i, l in enumerate(cm.get('content', '').split('\n')):
                        if l.strip().startswith('```mermaid'):
                            per[key].add(start + i)
counts = sorted(len(v) for v in per.values() if v)
n = len(counts)
q = lambda p: counts[min(n - 1, int(p * n))]
print('files', n, 'p50', q(.5), 'p90', q(.9), 'p99', q(.99), 'max', counts[-1])
c = Counter(counts)
for lim in (5, 10, 20, 30, 50):
    print('files with >= %d diagrams: %d (%.2f%%)' % (lim, sum(v for k, v in c.items() if k >= lim),
                                                     100.0 * sum(v for k, v in c.items() if k >= lim) / n))
