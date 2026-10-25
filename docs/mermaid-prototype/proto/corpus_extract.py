import json
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


