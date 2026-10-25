#!/usr/bin/env python3
"""Throwaway prototype of the Mermaid text renderer planned for folio.

It implements, in simplified form, the algorithms in the plan:
flowchart subset parser; DFS cycle breaking; longest-path ranking with label
ranks; barycenter ordering plus transpose; priority placement; channel routing
with tracks; side lanes for back edges; self-loops; mask painting; the sequence
subset parser, one-pass column solver, rows and frames; the fit ladder; the
outline and transcript fallbacks.

Not implemented: subgraph frames (members are laid out as if the subgraph
were absent), BT/RL mirroring (drawn as TD/LR), LR self-loops (dropped),
splitting segments when hard routing constraints form a cycle, ASCII charset.
"""
import heapq
import re
import sys
import unicodedata
from collections import defaultdict, deque


class Unsupported(Exception):
    pass


# ------------------------------------------------------------------ text
def cwidth(ch):
    o = ord(ch)
    cat = unicodedata.category(ch)
    if cat in ('Mn', 'Me', 'Cf') or 0x1160 <= o <= 0x11FF:
        return 0
    if unicodedata.east_asian_width(ch) in ('W', 'F'):
        return 2
    return 1


def width(s):
    return sum(cwidth(c) for c in s)


def graphemes(s):
    out = []
    for c in s:
        if out and cwidth(c) == 0:
            out[-1] += c
        else:
            out.append(c)
    return out


def break_word(word, w):
    out, cur, cw = [], '', 0
    for g in graphemes(word):
        gw = width(g)
        if cw + gw > w and cur:
            out.append(cur)
            cur, cw = '', 0
        cur += g
        cw += gw
    out.append(cur)
    return out


def wrap(text, w):
    w = max(1, w)
    lines = []
    for para in text.split('\n'):
        cur = ''
        for word in para.split():
            ww = width(word)
            if cur and width(cur) + 1 + ww <= w:
                cur += ' ' + word
                continue
            if cur:
                lines.append(cur)
                cur = ''
            if ww <= w:
                cur = word
            else:
                parts = break_word(word, w)
                lines.extend(parts[:-1])
                cur = parts[-1]
        lines.append(cur)
    return lines


def wrap_label(text, w):
    """Wrap a label; words up to 16 columns are never broken."""
    longest = max((width(x) for x in text.split()), default=0)
    return wrap(text, max(w, min(longest, 16)))


NAMED = {'quot': '"', 'amp': '&', 'lt': '<', 'gt': '>', 'nbsp': ' ', 'apos': "'",
         'hearts': '♥', 'copy': '©', 'rarr': '→', 'larr': '←'}


def decode(s):
    s = s.strip()
    md = False
    if len(s) >= 2 and s[0] == '"' and s[-1] == '"':
        s = s[1:-1]
        if len(s) >= 2 and s[0] == '`' and s[-1] == '`':
            s, md = s[1:-1], True
    s = re.sub(r'<br\s*/?>', '\n', s, flags=re.I)
    s = re.sub(r'</?[A-Za-z][^>]*>', '', s)

    def num(m):
        try:
            return chr(int(m.group(1)))
        except (ValueError, OverflowError):
            return m.group(0)
    s = re.sub(r'[#&]#?(\d+);', num, s)
    s = re.sub(r'[#&]([A-Za-z]+);', lambda m: NAMED.get(m.group(1).lower(), m.group(0)), s)
    s = re.sub(r'fa:fa-[\w-]+\s*', '', s)
    if md:
        s = s.replace('**', '').replace('__', '')
        s = re.sub(r'(?<!\w)[*_](\S[^*_]*?)[*_](?!\w)', r'\1', s)
    s = ''.join('�' if unicodedata.category(c) == 'Cc' and c != '\n' else c for c in s)
    return '\n'.join(' '.join(l.split()) for l in s.split('\n')).strip('\n')


# ------------------------------------------------------------- preamble
def strip_comment(l):
    q = False
    for k, c in enumerate(l):
        if c == '"':
            q = not q
        elif not q and l.startswith('%%', k):
            return l[:k]
    return l


def preamble(src):
    lines = src.replace('\r', '').split('\n')
    i = 0
    while i < len(lines) and not lines[i].strip():
        i += 1
    title = None
    if i < len(lines) and lines[i].strip() == '---':
        j = i + 1
        while j < len(lines) and lines[j].strip() != '---':
            m = re.match(r'title:\s*(.*)$', lines[j])
            if m:
                title = m.group(1).strip().strip('"\'')
            j += 1
        if j >= len(lines):
            raise Unsupported('front matter without a closing ---')
        i = j + 1
    body = []
    in_dir = False
    for l in lines[i:]:
        s = l.strip()
        if in_dir:
            if '}%%' in s:
                in_dir = False
            continue
        if s.startswith('%%{'):
            if '}%%' not in s:
                in_dir = True
            continue
        if s.startswith('%%'):
            continue
        body.append(strip_comment(l))
    return title, body


ENTITY = re.compile(r'[#&]#?\w+;')


def statements(body):
    out = []
    for line in body:
        prot = ENTITY.sub(lambda m: m.group(0).replace(';', '\x00'), line)
        depth, q, cur = 0, False, ''
        for c in prot:
            if c == '"':
                q = not q
            elif not q and c in '[({':
                depth += 1
            elif not q and c in '])}':
                depth = max(0, depth - 1)
            if c == ';' and not q and depth == 0:
                out.append(cur)
                cur = ''
            else:
                cur += c
        out.append(cur)
    return [s.replace('\x00', ';').strip() for s in out if s.strip()]


# ------------------------------------------------------------- canvas
N, S, W, E = 1, 2, 4, 8


def build_box_table():
    tab = {}
    for cp in range(0x2500, 0x2580):
        name = unicodedata.name(chr(cp), '')
        rest = name[len('BOX DRAWINGS '):]
        if any(x in rest for x in ('DASH', 'DOUBLE', 'ARC', 'DIAGONAL')):
            continue
        parts = rest.split(' AND ')
        w0 = parts[0].split()
        lead = w0[0] if w0[0] in ('LIGHT', 'HEAVY') else None
        dirs = {}
        for part in parts:
            ws = part.split()
            wt = None
            if ws[0] in ('LIGHT', 'HEAVY'):
                wt, ws = ws[0], ws[1:]
            elif ws[-1] in ('LIGHT', 'HEAVY'):
                wt, ws = ws[-1], ws[:-1]
            wt = wt or lead
            for wd in ws:
                for d in {'UP': 'N', 'DOWN': 'S', 'LEFT': 'W', 'RIGHT': 'E',
                          'HORIZONTAL': 'WE', 'VERTICAL': 'NS'}[wd]:
                    dirs[d] = wt
        tab[tuple(dirs.get(d) for d in 'NSWE')] = chr(cp)
    return tab


BOX = build_box_table()


class Canvas:
    def __init__(self):
        self.light = defaultdict(int)
        self.heavy = defaultdict(int)
        self.solid = defaultdict(int)
        self.dotted = defaultdict(int)
        self.glyphs = {}
        self.text = {}
        self.intended = set()
        self.violations = 0

    def line(self, y, x, bits, stroke='solid'):
        if y < 0 or x < 0:
            raise ValueError('negative coordinate')
        if stroke == 'thick':
            self.heavy[(y, x)] |= bits
        else:
            self.light[(y, x)] |= bits
        if stroke == 'dotted':
            self.dotted[(y, x)] += 1
        else:
            self.solid[(y, x)] += 1

    def glyph(self, y, x, ch):
        self.glyphs[(y, x)] = ch

    def put_text(self, y, x, s, over_line=False):
        if over_line:
            self.intended.update((y, x + k) for k in range(width(s)))
        for g in graphemes(s):
            gw = width(g)
            self.text[(y, x)] = g
            for k in range(1, gw):
                self.text[(y, x + k)] = ''
            x += max(gw, 1)
        return x

    def cells(self):
        return set(self.light) | set(self.heavy) | set(self.glyphs) | set(self.text)

    def char(self, y, x):
        if (y, x) in self.text:
            return self.text[(y, x)]
        if (y, x) in self.glyphs:
            return self.glyphs[(y, x)]
        lb, hb = self.light.get((y, x), 0), self.heavy.get((y, x), 0)
        bits = lb | hb
        if not bits:
            return ' '
        if self.dotted.get((y, x)) and not self.solid.get((y, x)):
            if bits in (N | S, N, S):
                return '┇' if hb else '┆'
            if bits in (W | E, W, E):
                return '┅' if hb else '┄'
        if bits in (N, S):
            lb, hb = (N | S if lb else 0), (N | S if hb else 0)
        if bits in (W, E):
            lb, hb = (W | E if lb else 0), (W | E if hb else 0)
        key = tuple('HEAVY' if hb & b else ('LIGHT' if lb & b else None) for b in (N, S, W, E))
        if key in BOX:
            return BOX[key]
        key = tuple('LIGHT' if (lb | hb) & b else None for b in (N, S, W, E))
        return BOX.get(key, '?')

    def render(self):
        cs = self.cells()
        if not cs:
            return []
        for (y, x) in self.light.keys() | self.heavy.keys():
            if (y, x) in self.text and self.text[(y, x)] not in (' ', '') and (y, x) not in self.intended:
                self.violations += 1
        h = max(y for y, _ in cs) + 1
        w = max(x for _, x in cs) + 1
        rows = []
        for y in range(h):
            out = []
            for x in range(w):
                c = self.char(y, x)
                out.append(c)
            rows.append(''.join(out).rstrip())
        return rows


# ------------------------------------------------------------- flowchart
DIRS = {'TB': 'TD', 'TD': 'TD', 'BT': 'BT', 'RL': 'RL', 'LR': 'LR'}
SHAPES = [('(((', [')))'], 'dcircle'), ('((', ['))'], 'circle'), ('([', ['])'], 'stadium'),
          ('[[', [']]'], 'subroutine'), ('[(', [')]'], 'cylinder'), ('[/', ['/]', '\\]'], 'slant'),
          ('[\\', ['\\]', '/]'], 'slant'), ('{{', ['}}'], 'hexagon'), ('(', [')'], 'round'),
          ('[', [']'], 'rect'), ('{', ['}'], 'decision'), ('>', [']'], 'asym')]
NEW_SHAPES = {'rect': 'rect', 'proc': 'rect', 'process': 'rect', 'rectangle': 'rect',
              'rounded': 'round', 'event': 'round', 'stadium': 'stadium', 'pill': 'stadium',
              'terminal': 'stadium', 'subproc': 'subroutine', 'subprocess': 'subroutine',
              'subroutine': 'subroutine', 'fr-rect': 'subroutine', 'cyl': 'cylinder',
              'cylinder': 'cylinder', 'database': 'cylinder', 'db': 'cylinder', 'circle': 'circle',
              'circ': 'circle', 'dbl-circ': 'dcircle', 'double-circle': 'dcircle',
              'diamond': 'decision', 'diam': 'decision', 'decision': 'decision', 'question': 'decision',
              'hex': 'hexagon', 'hexagon': 'hexagon', 'prepare': 'hexagon', 'lean-r': 'slant',
              'lean-l': 'slant', 'trap-b': 'slant', 'trap-t': 'slant', 'text': 'bare'}
IGNORED = ('classDef', 'class', 'style', 'linkStyle', 'click', 'accTitle', 'accDescr', 'title')


class FlowGraph:
    def __init__(self):
        self.dir = 'TD'
        self.nodes = []
        self.ids = {}
        self.edges = []
        self.clusters = {}
        self.cluster_depth = 0
        self.simplified = set()
        self.title = None


def skip(s, i):
    while i < len(s) and s[i] in ' \t':
        i += 1
    return i


def read_id(s, i):
    j = i
    while j < len(s):
        c = s[j]
        if c.isalnum() or c == '_' or (ord(c) > 127 and not c.isspace() and unicodedata.category(c)[0] in 'LNSM'):
            j += 1
        elif c in '-.' and j > i and j + 1 < len(s) and (s[j + 1].isalnum() or s[j + 1] == '_'):
            j += 1
        else:
            break
    return s[i:j], j


def get_node(g, nid, cluster):
    if nid == 'end':
        raise Unsupported('lowercase "end" used as a node name')
    k = g.ids.get(nid)
    if k is None:
        k = len(g.nodes)
        g.ids[nid] = k
        g.nodes.append({'id': nid, 'label': nid, 'shape': 'rect', 'cluster': cluster,
                        'slant': None, 'explicit': False})
    return k


def read_shape_text(s, i, closers):
    t = i
    while t < len(s) and s[t] == ' ':
        t += 1
    if t < len(s) and s[t] == '"':
        e = s.find('"', t + 1)
        if e < 0:
            raise Unsupported('unterminated quote')
        after = e + 1
        while after < len(s) and s[after] == ' ':
            after += 1
        for c in closers:
            if s.startswith(c, after):
                return s[t:e + 1], after + len(c), c
        raise Unsupported('text after a quoted label')
    best = None
    for c in closers:
        e = s.find(c, i)
        if e >= 0 and (best is None or e < best[0]):
            best = (e, c)
    if best is None:
        raise Unsupported('unterminated node shape')
    return s[i:best[0]], best[0] + len(best[1]), best[1]


def parse_node(s, i, g, cluster):
    i = skip(s, i)
    nid, j = read_id(s, i)
    if not nid:
        raise Unsupported('expected a node at "%s"' % s[i:i + 12])
    k = get_node(g, nid, cluster)
    node = g.nodes[k]
    if s.startswith('@{', j):
        e = s.find('}', j)
        if e < 0:
            raise Unsupported('unterminated @{')
        meta = s[j + 2:e]
        j = e + 1
        for key, val in re.findall(r'(\w+)\s*:\s*("[^"]*"|[^,]+)', meta):
            val = val.strip().strip('"')
            if key == 'shape':
                sh = NEW_SHAPES.get(val)
                if sh is None:
                    node['shape'] = 'rect'
                    g.simplified.add('shape')
                else:
                    node['shape'] = sh
                node['explicit'] = True
            elif key == 'label':
                node['label'] = decode('"%s"' % val)
                node['explicit'] = True
    else:
        for op, closers, shape in SHAPES:
            if s.startswith(op, j):
                text, j2, closer = read_shape_text(s, j + len(op), closers)
                node['label'] = decode(text)
                node['shape'] = shape
                node['explicit'] = True
                if shape == 'slant':
                    node['slant'] = (op[1], closer[0])
                elif shape == 'asym':
                    node['slant'] = ('>', '│')
                j = j2
                break
    if s.startswith(':::', j):
        m = re.match(r':::[\w-]+', s[j:])
        j += m.end() if m else 3
    return k, j


LINK = re.compile(r'(?P<lh>[<ox])?(?P<body>-{2,}|={2,}|-\.+-|~~~)(?P<rh>[>ox])?')
MID = re.compile(r'(?P<lh><)?(?P<op>--|==|-\.)(?=[ \t"]|[^-.>=ox|\s])')
HEAD = {'<': 'arrow', '>': 'arrow', 'o': 'circle', 'x': 'cross', None: 'none', '': 'none'}


def parse_link(s, i):
    i = skip(s, i)
    m = re.match(r'[A-Za-z_][\w-]*@(?=[<ox]?(?:--|==|-\.|~~~))', s[i:])
    if m:
        i += m.end()
    m = MID.match(s, i)
    if m:
        op = m.group('op')
        pat = {'--': r'(-{2,})([>ox]?)', '==': r'(={2,})([>ox]?)', '-.': r'(\.+)-([>ox]?)'}[op]
        cm = re.compile(pat).search(s, m.end())
        if not cm:
            raise Unsupported('edge text without a closing arrow')
        text = s[m.end():cm.start()]
        rh = cm.group(2)
        if op == '-.':
            stroke, minlen = 'dotted', len(cm.group(1))
        else:
            stroke = 'thick' if op == '==' else 'solid'
            minlen = max(1, len(cm.group(1)) - 2 + (1 if rh else 0))
        return {'stroke': stroke, 'ha': HEAD[m.group('lh')], 'hb': HEAD[rh],
                'label': decode(text) or None, 'minlen': minlen}, cm.end()
    m = LINK.match(s, i)
    if not m:
        return None
    body, lh, rh = m.group('body'), m.group('lh'), m.group('rh')
    if body == '~~~':
        stroke, minlen = 'invisible', 1
    elif body[0] == '=':
        stroke, minlen = 'thick', max(1, len(body) - 2 + (1 if rh else 0))
    elif '.' in body:
        stroke, minlen = 'dotted', body.count('.')
    else:
        stroke, minlen = 'solid', max(1, len(body) - 2 + (1 if rh else 0))
    j = m.end()
    label = None
    lm = re.match(r'\s*\|([^|]*)\|', s[j:])
    if lm:
        label = decode(lm.group(1)) or None
        j += lm.end()
    return {'stroke': stroke, 'ha': HEAD[lh], 'hb': HEAD[rh], 'label': label, 'minlen': minlen}, j


def parse_group(s, i, g, cluster):
    ks = []
    while True:
        k, i = parse_node(s, i, g, cluster)
        ks.append(k)
        j = skip(s, i)
        if j < len(s) and s[j] == '&':
            i = j + 1
            continue
        return ks, i


def parse_flow(header, stmts):
    g = FlowGraph()
    parts = header.split()
    rest = ''
    if len(parts) > 1 and parts[1].rstrip(';') in DIRS:
        g.dir = DIRS[parts[1].rstrip(';')]
        rest = header.split(None, 2)[2] if len(parts) > 2 else ''
    elif len(parts) > 1:
        rest = header.split(None, 1)[1]
    else:
        g.dir = 'TD'
    if g.dir in ('BT', 'RL'):
        g.simplified.add('mirror')
    stack = []
    todo = ([rest] if rest.strip() else []) + stmts
    skipping = False
    for st in todo:
        if skipping:
            if '}' in st:
                skipping = False
            continue
        first = st.split(None, 1)[0]
        word = re.match(r'[A-Za-z]+', first)
        word = word.group(0) if word else ''
        if word in IGNORED and (first == word or first[len(word):][:1] in ':{'):
            if word == 'accDescr' and '{' in st and '}' not in st:
                skipping = True
            continue
        if word == 'subgraph' and first == 'subgraph':
            rest = st[len('subgraph'):].strip()
            m = re.match(r'([^\s\[\]"]+)\s*\[(.*)\]\s*$', rest)
            if m:
                cid, title = m.group(1), decode(m.group(2))
            elif rest.startswith('"'):
                cid = title = decode(rest)
            else:
                cid, title = rest, rest
            if not cid:
                raise Unsupported('subgraph without a name')
            g.clusters[cid] = {'title': title, 'parent': stack[-1] if stack else None, 'members': []}
            stack.append(cid)
            g.cluster_depth = max(g.cluster_depth, len(stack))
            continue
        if st == 'end':
            if not stack:
                raise Unsupported('"end" without a subgraph')
            stack.pop()
            continue
        if word == 'direction' and first == 'direction':
            if stack:
                g.simplified.add('subgraph direction')
            continue
        if re.match(r'^[\w-]+@\{', st) and re.search(r'\b(animate|animation|curve)\b', st):
            continue
        if re.match(r'^[\w-]+@\{', st) and re.search(r'view\s*:\s*collapsed', st):
            raise Unsupported('collapsed subgraphs')
        cluster = stack[-1] if stack else None
        left, i = parse_group(st, 0, g, cluster)
        while True:
            i = skip(st, i)
            if i >= len(st):
                break
            lk = parse_link(st, i)
            if lk is None:
                raise Unsupported('unexpected text "%s"' % st[i:i + 15])
            link, i = lk
            right, i = parse_group(st, i, g, cluster)
            for a in left:
                for b in right:
                    e = dict(link)
                    e['a'], e['b'] = a, b
                    g.edges.append(e)
            left = right
    if stack:
        raise Unsupported('subgraph without "end"')
    for nd in g.nodes:
        if nd['cluster']:
            g.clusters[nd['cluster']]['members'].append(g.ids[nd['id']])
    # A subgraph used as an edge end attaches to its first member (simplified).
    for cid, cl in g.clusters.items():
        k = g.ids.get(cid)
        if k is None or g.nodes[k]['explicit']:
            continue
        members = [m for m in cl['members'] if m != k]
        if not members:
            raise Unsupported('edge to an empty subgraph')
        inside = set(members)
        target = next((m for m in members if not any(e['b'] == m and e['a'] in inside for e in g.edges)), members[0])
        for e in g.edges:
            if e['a'] == k:
                e['a'] = target
            if e['b'] == k:
                e['b'] = target
        g.nodes[k]['dead'] = True
        g.simplified.add('subgraph edge')
    if g.cluster_depth:
        g.simplified.add('subgraph')
    if not [n for n in g.nodes if not n.get('dead')]:
        raise Unsupported('no nodes')
    return g


class Prof:
    def __init__(self, name, label_w, node_gap, lr_chan, direction):
        self.name, self.label_w, self.node_gap, self.lr_chan, self.dir = name, label_w, node_gap, lr_chan, direction


def flow_prepare(g):
    """Ranks, chains, order and lanes: done once per diagram."""
    nodes = [k for k, n in enumerate(g.nodes) if not n.get('dead')]
    ED = g.edges
    selfloops = [k for k, e in enumerate(ED) if e['a'] == e['b'] and e['stroke'] != 'invisible']
    real = [k for k, e in enumerate(ED) if e['a'] != e['b']]
    out = defaultdict(list)
    for k in real:
        out[ED[k]['a']].append(k)
    state = defaultdict(int)
    back = set()
    for root in nodes:
        if state[root]:
            continue
        state[root] = 1
        stack = [(root, iter(out[root]))]
        while stack:
            u, it = stack[-1]
            for k in it:
                v = ED[k]['b']
                if state[v] == 1:
                    back.add(k)
                elif state[v] == 0:
                    state[v] = 1
                    stack.append((v, iter(out[v])))
                    break
            else:
                state[u] = 2
                stack.pop()
    has_label = any(ED[k]['label'] for k in real if ED[k]['stroke'] != 'invisible')
    f = 2 if has_label else 1
    dag = []
    for k in real:
        a, b = ED[k]['a'], ED[k]['b']
        rev = k in back
        u, v = (b, a) if rev else (a, b)
        dag.append((u, v, ED[k]['minlen'] * f, k, rev))
    succ = defaultdict(list)
    indeg = defaultdict(int)
    for (u, v, ml, k, rev) in dag:
        succ[u].append((v, ml))
        indeg[v] += 1
    rank = {v: 0 for v in nodes}
    heap = [v for v in nodes if indeg[v] == 0]
    heapq.heapify(heap)
    topo = []
    deg = dict(indeg)
    while heap:
        u = heapq.heappop(heap)
        topo.append(u)
        for v, ml in succ[u]:
            rank[v] = max(rank[v], rank[u] + ml)
            deg[v] -= 1
            if deg[v] == 0:
                heapq.heappush(heap, v)
    for u in topo:
        if indeg[u] == 0 and succ[u]:
            rank[u] = min(rank[v] - ml for v, ml in succ[u])
    lo = min(rank.values())
    for v in rank:
        rank[v] -= lo
    elems = []

    def new(kind, r, node=None, edge=None):
        elems.append({'kind': kind, 'rank': r, 'node': node, 'edge': edge})
        return len(elems) - 1
    node_elem = {v: new('node', rank[v], node=v) for v in nodes}
    chains = {}
    info = {}
    for (u, v, ml, k, rev) in dag:
        info[k] = (u, v, rev)
        if ED[k]['stroke'] == 'invisible':
            continue
        r1, r2 = rank[u], rank[v]
        ch = [node_elem[u]]
        for r in range(r1 + 1, r2):
            if ED[k]['label'] and r == r1 + 1 and f == 2:
                ch.append(new('label', r, edge=k))
            else:
                ch.append(new('dummy', r, edge=k))
        ch.append(node_elem[v])
        chains[k] = ch
    R = max(rank.values()) if rank else 0
    order = defaultdict(list)
    placed = set()

    def place(e):
        if e not in placed:
            placed.add(e)
            order[elems[e]['rank']].append(e)
    from_node = defaultdict(list)
    for k in sorted(chains):
        from_node[elems[chains[k][0]]['node']].append(k)
    seen = set()
    q = deque(v for v in nodes if indeg[v] == 0)
    seen.update(q)
    while q:
        u = q.popleft()
        place(node_elem[u])
        for k in from_node[u]:
            for e in chains[k][1:-1]:
                place(e)
            t = elems[chains[k][-1]]['node']
            place(chains[k][-1])
            if t not in seen:
                seen.add(t)
                q.append(t)
    for v in nodes:
        place(node_elem[v])

    def links():
        up, down, link_edge = defaultdict(list), defaultdict(list), {}
        for k, ch in chains.items():
            for x, y in zip(ch, ch[1:]):
                down[x].append(y)
                up[y].append(x)
                link_edge[(x, y)] = k
        return up, down, link_edge
    up, down, link_edge = links()

    def crossings_between(r):
        P = {e: i for i, e in enumerate(order[r + 1])}
        Pu = {e: i for i, e in enumerate(order[r])}
        pairs = sorted((Pu[x], P[y]) for x in order[r] for y in down[x])
        c = 0
        bs = [b for _, b in pairs]
        # simple O(m^2) for small m, fine for a prototype
        for i in range(len(bs)):
            for j in range(i + 1, len(bs)):
                if bs[j] < bs[i] and pairs[j][0] != pairs[i][0]:
                    c += 1
        return c

    def total():
        return sum(crossings_between(r) for r in range(R))
    best = {r: list(order[r]) for r in range(R + 1)}
    bestc = total()
    stale = 0
    for it in range(8):
        down_sweep = it % 2 == 0
        P = {e: i for r in range(R + 1) for i, e in enumerate(order[r])}
        ranks = range(1, R + 1) if down_sweep else range(R - 1, -1, -1)
        for r in ranks:
            key = {}
            for e in order[r]:
                nb = up[e] if down_sweep else down[e]
                key[e] = sum(P[x] for x in nb) / len(nb) if nb else P[e]
            order[r].sort(key=lambda e: key[e])
            for i, e in enumerate(order[r]):
                P[e] = i
        c = total()
        if c < bestc:
            best, bestc, stale = {r: list(order[r]) for r in range(R + 1)}, c, 0
        else:
            stale += 1
            if stale >= 4:
                break
    for r in range(R + 1):
        order[r] = best[r]
    # transpose
    for _ in range(20):
        changed = False
        P = {e: i for r in range(R + 1) for i, e in enumerate(order[r])}
        for r in range(R + 1):
            lst = order[r]
            for i in range(len(lst) - 1):
                a, b = lst[i], lst[i + 1]

                def cx(a, b):
                    c = 0
                    for nb in (up, down):
                        for pa in nb[a]:
                            for pb in nb[b]:
                                if P[pa] > P[pb]:
                                    c += 1
                    return c
                if cx(a, b) > cx(b, a):
                    lst[i], lst[i + 1] = b, a
                    P[a], P[b] = i + 1, i
                    changed = True
        if not changed:
            break
    # lanes for back edges
    # Lanes for back edges, shortest span first. A node can send one lane;
    # several lanes may arrive at the same node (they merge at its arrowhead).
    loopers = set(ED[k]['a'] for k in selfloops)
    cand = sorted((k for k in back if k in chains),
                  key=lambda k: (abs(rank[ED[k]['a']] - rank[ED[k]['b']]), k))
    rejected = set()
    while True:
        lane_elems = set()
        for k in cand:
            if k not in rejected:
                lane_elems.update(chains[k][1:-1])

        def last(e):
            lst = [x for x in order[elems[e]['rank']] if x not in lane_elems]
            return bool(lst) and lst[-1] == e
        sources, targets, accepted = set(), set(), []
        changed = False
        for k in cand:
            if k in rejected:
                continue
            v, u = ED[k]['a'], ED[k]['b']
            ok = (last(node_elem[v]) and last(node_elem[u]) and v not in loopers and u not in loopers
                  and v not in sources and v not in targets and u not in sources)
            if ok:
                accepted.append(k)
                sources.add(v)
                targets.add(u)
            else:
                rejected.add(k)
                changed = True
        if not changed:
            break
    lanes = accepted
    for k in lanes:
        for e in chains[k][1:-1]:
            order[elems[e]['rank']].remove(e)
        del chains[k]
    up, down, link_edge = links()
    return {'g': g, 'rank': rank, 'R': R, 'elems': elems, 'order': order, 'chains': chains,
            'info': info, 'up': up, 'down': down, 'link_edge': link_edge, 'lanes': lanes,
            'selfloops': selfloops, 'node_elem': node_elem, 'f': f, 'back': back}


def node_box(nd, prof, TD, selfloop):
    lines = wrap_label(nd['label'], prof.label_w) if nd['label'] else ['']
    lw = max(width(l) for l in lines)
    shape = nd['shape']
    inner = lw if shape == 'bare' else lw + 2
    if TD and inner % 2 == 0:
        inner += 1
    w = inner + (0 if shape == 'bare' else 2) + (2 if shape == 'subroutine' else 0)
    h = len(lines) + (0 if shape == 'bare' else 2) + (1 if shape == 'cylinder' else 0)
    if TD and selfloop:
        h = max(h, len(lines) + 3)
    if not TD and h % 2 == 0:
        h += 1
    return {'lines': lines, 'lw': lw, 'w': w, 'h': h, 'shape': shape, 'slant': nd['slant']}


ARROW = {('TD', 1): '▾', ('TD', -1): '▴', ('LR', 1): '▸', ('LR', -1): '◂'}
OTHER_HEAD = {'circle': 'o', 'cross': '✕'}


def flow_draw(prep, prof):
    g, ED, elems, order, R = prep["g"], prep["g"].edges, prep["elems"], prep["order"], prep["R"]
    up, down, link_edge, chains = prep['up'], prep['down'], prep['link_edge'], prep['chains']
    TD = prof.dir == 'TD'
    cv = Canvas()
    loop_nodes = defaultdict(list)
    for k in prep['selfloops']:
        loop_nodes[ED[k]['a']].append(k)
    boxes = {}
    for k, nd in enumerate(g.nodes):
        if nd.get('dead'):
            continue
        boxes[k] = node_box(nd, prof, TD, bool(loop_nodes.get(k)) and TD)
    labels = {}
    for e, el in enumerate(elems):
        if el['kind'] == 'label':
            lines = wrap_label(ED[el['edge']]['label'], prof.label_w)
            labels[e] = (lines, max(width(l) for l in lines))
    # heads at the upper and lower end of each chain
    def ends(k):
        u, v, rev = prep['info'][k]
        e = ED[k]
        return (e['hb'], e['ha']) if rev else (e['ha'], e['hb'])
    # ports
    off = {}
    P = {e: i for r in range(R + 1) for i, e in enumerate(order[r])}
    for k_node, ne in prep['node_elem'].items():
        b = boxes[k_node]
        for side, nbmap in (('far', down), ('near', up)):
            groups = {}
            for y in nbmap[ne]:
                k = link_edge[(ne, y) if side == 'far' else (y, ne)]
                hu, hl = ends(k)
                key = (ED[k]['stroke'], hu if side == 'far' else hl)
                groups.setdefault(key, []).append(y)
            if not groups:
                continue
            keys = sorted(groups, key=lambda key: sum(P[y] for y in groups[key]) / len(groups[key]))
            n = len(keys)
            ext = b['w'] if TD else b['h']
            if n > 1 and ext - 2 < 2 * n - 1:
                ext = 2 * n + 1
                if TD:
                    b['w'] = ext
                else:
                    b['h'] = ext
            c = (ext - 1) // 2
            for i, key in enumerate(keys):
                p = c if n == 1 else int(round((i + 1) * (ext - 1) / (n + 1)))
                p = min(max(p, 1), ext - 2)
                for y in groups[key]:
                    off[(ne, y)] = p - c
    # second pass in case boxes were widened after offsets of earlier groups
    lo, hi, extr = {}, {}, {}
    for e, el in enumerate(elems):
        if el['kind'] == 'node':
            b = boxes[el['node']]
            ext = b['w'] if TD else b['h']
            lo[e] = (ext - 1) // 2
            hi[e] = ext - 1 - lo[e]
            extr[e] = b['h'] if TD else b['w']
            if TD and loop_nodes.get(el['node']):
                lw = max((width(ED[k]['label'] or '') for k in loop_nodes[el['node']]), default=0)
                hi[e] += 3 + (1 + lw if lw else 0)
        elif el['kind'] == 'label':
            lines, lw = labels[e]
            ext = lw if TD else len(lines)
            lo[e] = (ext - 1) // 2
            hi[e] = ext - 1 - lo[e]
            extr[e] = len(lines) if TD else lw + 2
        else:
            lo[e] = hi[e] = 0
            extr[e] = 0

    def gap(a, b):
        ka, kb = elems[a]['kind'], elems[b]['kind']
        if ka == 'node' and kb == 'node':
            return prof.node_gap if TD else 1
        if 'label' in (ka, kb):
            return 2 if TD else 1
        return 1

    def sep(a, b):
        return hi[a] + gap(a, b) + lo[b] + 1
    pos = {}
    for r in range(R + 1):
        x = None
        for i, e in enumerate(order[r]):
            x = lo[e] if i == 0 else x + sep(order[r][i - 1], e)
            pos[e] = x

    def port(e, nb):
        return pos[e] + off.get((e, nb), 0)

    def desired(e, nbs):
        vals = sorted(port(n, e) - off.get((e, n), 0) for n in nbs)
        m = len(vals)
        return vals[m // 2] if m % 2 else (vals[m // 2 - 1] + vals[m // 2]) // 2

    def place_rank(r, ref):
        lst = order[r]
        n = len(lst)
        nbmap = up if ref == 'up' else down
        prio = [10 ** 6 if elems[e]['kind'] != 'node' else len(nbmap[e]) for e in lst]
        done = [False] * n
        for i in sorted(range(n), key=lambda i: (-prio[i], i)):
            e = lst[i]
            nbs = down[e] if elems[e]['kind'] == 'label' else nbmap[e]
            if not nbs:
                done[i] = True
                continue
            d = desired(e, nbs)
            x = pos[e]
            if d > x:
                lim, acc = d, 0
                for j in range(i + 1, n):
                    acc += sep(lst[j - 1], lst[j])
                    if done[j]:
                        lim = min(lim, pos[lst[j]] - acc)
                        break
                pos[e] = max(x, lim)
                for j in range(i + 1, n):
                    need = pos[lst[j - 1]] + sep(lst[j - 1], lst[j])
                    if pos[lst[j]] < need:
                        pos[lst[j]] = need
                    else:
                        break
            elif d < x:
                lim, acc = d, 0
                for j in range(i - 1, -1, -1):
                    acc += sep(lst[j], lst[j + 1])
                    if done[j]:
                        lim = max(lim, pos[lst[j]] + acc)
                        break
                pos[e] = min(x, lim)
                for j in range(i - 1, -1, -1):
                    need = pos[lst[j + 1]] - sep(lst[j], lst[j + 1])
                    if pos[lst[j]] > need:
                        pos[lst[j]] = need
                    else:
                        break
            done[i] = True
    for _ in range(3):
        for r in range(1, R + 1):
            place_rank(r, 'up')
        for r in range(R - 1, -1, -1):
            place_rank(r, 'down')
    for r in range(1, R + 1):
        place_rank(r, 'up')
    minx = min((pos[e] - lo[e] for e in pos), default=0)
    for e in pos:
        pos[e] -= minx
    # rank-axis coordinates and channel routing
    thick = [max((extr[e] for e in order[r]), default=0) for r in range(R + 1)]
    RS, CS, H = [0] * (R + 1), [0] * (R + 1), [0] * (R + 1)
    plans = []
    y = 0
    for r in range(R + 1):
        RS[r] = y
        CS[r] = y + thick[r]
        y = CS[r]
        if r == R:
            break
        pieces = []
        for e in order[r]:
            for t in down[e]:
                k = link_edge[(e, t)]
                hu, hl = ends(k)
                pieces.append({'e1': e, 'e2': t, 'top': port(e, t), 'bot': port(t, e), 'k': k,
                               'stroke': ED[k]['stroke'],
                               'at': hu if elems[e]['kind'] == 'node' and hu != 'none' else None,
                               'ab': hl if elems[t]['kind'] == 'node' and hl != 'none' else None})
        groups = {}
        for p in pieces:
            groups.setdefault(('t', p['e1'], p['top']), []).append(p)
        segs, singles = [], []
        for key, ps in groups.items():
            (segs if len(ps) > 1 else singles).append(ps)
        bot = {}
        for ps in singles:
            p = ps[0]
            bot.setdefault((p['e2'], p['bot']), []).append(p)
        segs += list(bot.values())
        seginfo = []
        for ps in segs:
            tops = {p['top'] for p in ps}
            bots = {p['bot'] for p in ps}
            allx = tops | bots
            seginfo.append({'ps': ps, 'tops': tops, 'bots': bots, 'l': min(allx), 'r': max(allx),
                            'straight': len(allx) == 1})
        ns = [s for s in seginfo if not s['straight']]
        INF = 10 ** 9
        above = defaultdict(dict)

        def cost(a, b):
            return (sum(1 for x in b['tops'] if a['l'] < x < a['r']) +
                    sum(1 for x in a['bots'] if b['l'] < x < b['r']))
        for i in range(len(ns)):
            for j in range(i + 1, len(ns)):
                a, b = ns[i], ns[j]
                if a['r'] + 1 < b['l'] or b['r'] + 1 < a['l']:
                    continue
                hab, hba = bool(a['tops'] & b['bots']), bool(b['tops'] & a['bots'])
                if hab and not hba:
                    above[i][j] = INF
                elif hba and not hab:
                    above[j][i] = INF
                else:
                    cab, cba = cost(a, b), cost(b, a)
                    if cab <= cba:
                        above[i][j] = cba - cab + (INF if hab else 0)
                    else:
                        above[j][i] = cab - cba
        # break cycles: remove the lightest edge on each cycle found
        while True:
            color, parent, cyc = {}, {}, None

            def visit(s0):
                stack = [(s0, iter(list(above[s0])))]
                color[s0] = 1
                while stack:
                    u, it = stack[-1]
                    for v in it:
                        if color.get(v, 0) == 1:
                            path = [v]
                            x = u
                            while x != v:
                                path.append(x)
                                x = parent[x]
                            return list(reversed(path))
                        if color.get(v, 0) == 0:
                            color[v] = 1
                            parent[v] = u
                            stack.append((v, iter(list(above[v]))))
                            break
                    else:
                        color[u] = 2
                        stack.pop()
                return None
            for s0 in range(len(ns)):
                if color.get(s0, 0) == 0:
                    cyc = visit(s0)
                    if cyc:
                        break
            if not cyc:
                break
            ring = list(zip(cyc, cyc[1:] + cyc[:1]))
            a, b = min(ring, key=lambda ab: above[ab[0]].get(ab[1], 0))
            above[a].pop(b, None)
        track = [0] * len(ns)
        indeg2 = [0] * len(ns)
        for a in above:
            for b in above[a]:
                indeg2[b] += 1
        qq = deque(i for i in range(len(ns)) if indeg2[i] == 0)
        while qq:
            a = qq.popleft()
            for b in above[a]:
                track[b] = max(track[b], track[a] + 1)
                indeg2[b] -= 1
                if indeg2[b] == 0:
                    qq.append(b)
        T = max(track) + 1 if ns else 0
        stem = 1 if (T or any(p['at'] for p in pieces)) else 0
        bottom = 1 if any(elems[p['e2']]['kind'] == 'node' for p in pieces) else 0
        h = stem + T + bottom
        upper_nodes = any(elems[e]['kind'] != 'dummy' for e in order[r])
        lower_nodes = any(elems[e]['kind'] != 'dummy' for e in order[r + 1])
        if any(p['stroke'] in ('dotted', 'thick') and p['top'] == p['bot'] and elems[p['e2']]['kind'] == 'node'
               for p in pieces) and T == 0:
            h = max(h, 2 + stem)
        if TD:
            if any(elems[p['e1']]['kind'] == 'node' and elems[p['e2']]['kind'] == 'label' for p in pieces):
                h = max(h, 1)
            if upper_nodes and lower_nodes:
                h = max(h, 1)
        else:
            if pieces:
                h = max(h, prof.lr_chan)
            if upper_nodes and lower_nodes:
                h = max(h, 1)
        H[r] = h
        segtrack = {}
        for i, s in enumerate(ns):
            for p in s['ps']:
                segtrack[id(p)] = track[i]
        plans.append((r, pieces, segtrack, stem, T))
        y = CS[r] + h

    def scr(rr, oo):
        return (rr, oo) if TD else (oo, rr)
    D = {(1, 0): (S if TD else E), (-1, 0): (N if TD else W), (0, 1): (E if TD else S), (0, -1): (W if TD else N)}

    def apath(pts, stroke, start_bit=None, end_bit=None):
        seq = [pts[0]]
        for (r0, o0), (r1, o1) in zip(pts, pts[1:]):
            dr, do = (r1 > r0) - (r1 < r0), (o1 > o0) - (o1 < o0)
            rr, oo = r0, o0
            while (rr, oo) != (r1, o1):
                rr += dr
                oo += do
                if seq[-1] != (rr, oo):
                    seq.append((rr, oo))
        for i, (rr, oo) in enumerate(seq):
            bits = 0
            for j in (i - 1, i + 1):
                if 0 <= j < len(seq):
                    bits |= D[(seq[j][0] - rr, seq[j][1] - oo)]
            if i == 0 and start_bit is not None:
                bits |= D[start_bit]
            if i == len(seq) - 1 and end_bit is not None:
                bits |= D[end_bit]
            yy, xx = scr(rr, oo)
            cv.line(yy, xx, bits, stroke)

    def out_anchor(e):
        el = elems[e]
        r = el['rank']
        if el['kind'] == 'dummy' and thick[r] == 0:
            return CS[r]
        return RS[r] + (extr[e] if el['kind'] != 'dummy' else thick[r]) - 1

    def in_anchor(e):
        el = elems[e]
        r = el['rank']
        if el['kind'] == 'dummy' and thick[r] == 0:
            return RS[r] - 1
        return RS[r]
    for (r, pieces, segtrack, stem, T) in plans:
        for p in pieces:
            e1, e2 = p['e1'], p['e2']
            rs = out_anchor(e1)
            re_ = in_anchor(e2)
            sb = (-1, 0) if elems[e1]['kind'] in ('dummy', 'label') else None
            eb = (1, 0) if elems[e2]['kind'] in ('dummy', 'label') else None
            if elems[e1]['kind'] == 'label':
                rs += 1
            if elems[e2]['kind'] == 'label':
                re_ -= 1
            if p['at']:
                rs += 1
                yy, xx = scr(rs, p['top'])
                cv.glyph(yy, xx, ARROW[(prof.dir, -1)] if p['at'] == 'arrow' else OTHER_HEAD[p['at']])
            if p['ab']:
                re_ -= 1
                yy, xx = scr(re_, p['bot'])
                cv.glyph(yy, xx, ARROW[(prof.dir, 1)] if p['ab'] == 'arrow' else OTHER_HEAD[p['ab']])
            if p['top'] == p['bot']:
                if rs > re_:
                    continue
                pts = [(rs, p['top']), (re_, p['top'])]

            else:
                tr = CS[r] + stem + segtrack[id(p)]
                pts = [(rs, p['top']), (tr, p['top']), (tr, p['bot']), (re_, p['bot'])]
            apath(pts, p['stroke'], sb, eb)
    # dummies that pass through a band with thickness
    for e, el in enumerate(elems):
        if el['kind'] == 'dummy' and e in pos and thick[el['rank']] > 0 and e in up:
            r = el['rank']
            apath([(RS[r], pos[e]), (RS[r] + thick[r] - 1, pos[e])], ED[el['edge']]['stroke'], (-1, 0), (1, 0))
    # nodes
    rects = {}
    for k, ne in prep['node_elem'].items():
        b = boxes[k]
        r = elems[ne]['rank']
        if TD:
            x0, y0 = pos[ne] - lo[ne], RS[r]
        else:
            x0, y0 = RS[r], pos[ne] - lo[ne]
        rects[k] = (x0, y0, b['w'], b['h'])
        paint_box(cv, x0, y0, b)
    # labels
    for e, (lines, lw) in labels.items():
        if e not in pos:
            continue
        r = elems[e]['rank']
        if TD:
            for i, l in enumerate(lines):
                cv.put_text(RS[r] + i, pos[e] - lo[e] + (lw - width(l)) // 2, l)
        else:
            for i, l in enumerate(lines):
                yy = pos[e] - lo[e] + i
                txt = ' ' + l.center(lw) + ' ' if False else ' ' + l + ' ' * (lw - width(l) + 1)
                cv.put_text(yy, RS[r], txt)
    # self-loops (TD only)
    for k, ks in loop_nodes.items():
        if not TD:
            g.simplified.add('LR self-loop')
            continue
        x0, y0, w, h = rects[k]
        xr = x0 + w - 1
        ya, yb = y0 + 1, y0 + 2
        cv.line(ya, xr, E)
        cv.line(ya, xr + 1, W | E)
        cv.line(ya, xr + 2, W | S)
        cv.line(yb, xr + 2, N | W)
        cv.glyph(yb, xr + 1, '◂')
        lab = ED[ks[0]]['label']
        if lab:
            cv.put_text(ya, xr + 4, lab.replace('\n', ' '))
    # lanes for back edges, shortest span first
    def span(k):
        return abs(elems[prep['node_elem'][ED[k]['a']]]['rank'] - elems[prep['node_elem'][ED[k]['b']]]['rank'])
    for k in sorted(prep['lanes'], key=span):
        v, u = ED[k]['a'], ED[k]['b']
        vx, vy, vw, vh = rects[v]
        ux, uy, uw, uh = rects[u]
        lab = (ED[k]['label'] or '').replace('\n', ' ')
        lw = width(lab)
        stroke = ED[k]['stroke']
        head = ED[k]['hb']
        cells = cv.cells()
        if TD:
            ym_v, ym_u = vy + (vh - 1) // 2, uy + (uh - 1) // 2
            xr_v, xr_u = vx + vw - 1, ux + uw - 1
            far = max((x for (yy, x) in cells if ym_u <= yy <= ym_v), default=0)
            lane = max(far + 2, xr_v + 2 + (lw + 2 if lw else 0) + 1)
            pts = [(ym_v, xr_v), (ym_v, lane), (ym_u, lane), (ym_u, xr_u + (2 if head != 'none' else 1))]
            seq_paint(cv, pts, stroke, 0, W)
            if head != 'none':
                cv.glyph(ym_u, xr_u + 1, '◂' if head == 'arrow' else OTHER_HEAD[head])
            if lw:
                cv.put_text(ym_v, xr_v + 2, ' ' + lab + ' ', over_line=True)
        else:
            xm_v, xm_u = vx + (vw - 1) // 2, ux + (uw - 1) // 2
            yb_v, yb_u = vy + vh - 1, uy + uh - 1
            far = max((yy for (yy, x) in cells if xm_u <= x <= xm_v), default=0)
            lane = far + 2
            pts = [(yb_v, xm_v), (lane, xm_v), (lane, xm_u), (yb_u + (2 if head != 'none' else 1), xm_u)]
            seq_paint(cv, pts, stroke, 0, N)

            if head != 'none':
                cv.glyph(yb_u + 1, xm_u, '▴' if head == 'arrow' else OTHER_HEAD[head])
            if lw:
                cv.put_text(lane, xm_u + 2, ' ' + lab + ' ', over_line=True)

    rows = cv.render()
    return rows, cv.violations


def seq_paint(cv, pts, stroke, start_bit=0, end_bit=0):
    """Paint an orthogonal path in screen coordinates (y, x)."""
    seq = [pts[0]]
    for (y0, x0), (y1, x1) in zip(pts, pts[1:]):
        dy, dx = (y1 > y0) - (y1 < y0), (x1 > x0) - (x1 < x0)
        y, x = y0, x0
        while (y, x) != (y1, x1):
            y += dy
            x += dx
            if seq[-1] != (y, x):
                seq.append((y, x))
    DD = {(1, 0): S, (-1, 0): N, (0, 1): E, (0, -1): W}
    for i, (y, x) in enumerate(seq):
        bits = 0
        for j in (i - 1, i + 1):
            if 0 <= j < len(seq):
                bits |= DD[(seq[j][0] - y, seq[j][1] - x)]
        if i == 0:
            bits |= start_bit
        if i == len(seq) - 1:
            bits |= end_bit
        cv.line(y, x, bits, stroke)


def paint_box(cv, x0, y0, b):
    w, h, shape, lines = b['w'], b['h'], b['shape'], b['lines']
    x1, y1 = x0 + w - 1, y0 + h - 1
    if shape != 'bare':
        for x in range(x0 + 1, x1):
            cv.line(y0, x, W | E)
            cv.line(y1, x, W | E)
        for y in range(y0 + 1, y1):
            cv.line(y, x0, N | S)
            cv.line(y, x1, N | S)
        if shape in ('round', 'stadium', 'circle', 'dcircle', 'cylinder'):
            cv.glyph(y0, x0, '╭')
            cv.glyph(y0, x1, '╮')
            cv.glyph(y1, x0, '╰')
            cv.glyph(y1, x1, '╯')
        elif shape in ('decision', 'hexagon'):
            cv.glyph(y0, x0, '╱')
            cv.glyph(y0, x1, '╲')
            cv.glyph(y1, x0, '╲')
            cv.glyph(y1, x1, '╱')
        else:
            cv.line(y0, x0, S | E)
            cv.line(y0, x1, S | W)
            cv.line(y1, x0, N | E)
            cv.line(y1, x1, N | W)
        if shape == 'subroutine':
            for y in range(y0, y1 + 1):
                cv.line(y, x0 + 1, (S if y == y0 else N if y == y1 else N | S))
                cv.line(y, x1 - 1, (S if y == y0 else N if y == y1 else N | S))
        if shape == 'cylinder':
            cv.line(y0 + 1, x0, E)
            cv.line(y0 + 1, x1, W)
            for x in range(x0 + 1, x1):
                cv.line(y0 + 1, x, W | E)
    top = y0 + (0 if shape == 'bare' else 1) + (1 if shape == 'cylinder' else 0)
    inner_l = x0 + (0 if shape == 'bare' else 1) + (1 if shape == 'subroutine' else 0)
    inner_w = w - (0 if shape == 'bare' else 2) - (2 if shape == 'subroutine' else 0)
    for y in range(top, y1 if shape != 'bare' else y1 + 1):
        for x in range(inner_l, inner_l + inner_w):
            cv.put_text(y, x, ' ')
    mid = top + (len(lines) - 1) // 2 if shape in ('decision', 'hexagon') else None
    for i, l in enumerate(lines):
        cv.put_text(top + i, inner_l + (inner_w - width(l)) // 2, l)
    for y in range(top, top + len(lines)):
        if shape in ('stadium', 'circle', 'dcircle'):
            cv.glyph(y, x0, '(')
            cv.glyph(y, x1, ')')
        elif shape in ('decision', 'hexagon') and y == mid:
            cv.glyph(y, x0, '<')
            cv.glyph(y, x1, '>')
        elif shape in ('slant', 'asym') and b['slant']:
            cv.glyph(y, x0, b['slant'][0])
            if b['slant'][1] != '│':
                cv.glyph(y, x1, b['slant'][1])


def flow_outline(g, w):
    def name(k):
        return g.nodes[k]['label'].replace('\n', ' ') or g.nodes[k]['id']
    out = []
    for e in g.edges:
        if e['stroke'] == 'invisible':
            continue
        line = {'solid': '─', 'dotted': '┄', 'thick': '━'}[e['stroke']]
        mid = line + (e['label'].replace('\n', ' ') + line if e['label'] else '')
        right = '▸' if e['hb'] == 'arrow' else ('' if e['hb'] == 'none' else OTHER_HEAD[e['hb']])
        left = '◂' if e['ha'] == 'arrow' else ''
        out.append('%s %s%s%s %s' % (name(e['a']), left, mid, right or line, name(e['b'])))
    linked = {e['a'] for e in g.edges} | {e['b'] for e in g.edges}
    for k, nd in enumerate(g.nodes):
        if k not in linked and not nd.get('dead'):
            out.append(name(k))
    rows = []
    for l in out:
        ws = wrap(l, w)
        rows.append(ws[0])
        for c in wrap(' '.join(ws[1:]), w - 2) if len(ws) > 1 else []:
            rows.append('  ' + c)
    return rows


# ------------------------------------------------------------- sequence
ARROWS = ['<<-->>', '<<->>', '-->>', '->>', '--x', '-x', '--)', '-)', '-->', '->']
MSG = re.compile(r'^(?P<a>[^:]+?)\s*(?P<arrow><<-->>|<<->>|-->>|->>|--x|-x|--\)|-\)|-->|->)\s*(?P<act>[+-]?)\s*(?P<b>[^:]+?)\s*:\s*(?P<text>.*)$')
BLOCKS = ('loop', 'alt', 'opt', 'par', 'critical', 'break', 'rect')


class Seq:
    def __init__(self):
        self.parts = []
        self.ids = {}
        self.events = []
        self.autonumber = None
        self.simplified = set()


def seq_part(sd, pid, label=None, kind='participant'):
    pid = pid.strip()
    if not pid or ' ' in pid and label is None and pid not in sd.ids:
        if ' ' in pid:
            pass
    k = sd.ids.get(pid)
    if k is None:
        k = len(sd.parts)
        sd.ids[pid] = k
        sd.parts.append({'id': pid, 'label': label if label is not None else pid, 'kind': kind})
    elif label is not None:
        sd.parts[k]['label'] = label
    return k


def parse_seq(header, stmts):
    sd = Seq()
    rest = header[len('sequenceDiagram'):].strip()
    todo = ([rest] if rest else []) + stmts
    stack = []
    for st in todo:
        low = st.lower()
        first = st.split(None, 1)[0]
        if first in ('accTitle', 'accDescr') or first.startswith(('accTitle:', 'accDescr:')) or first == 'title' or first.startswith('title:'):
            continue
        m = re.match(r'^(participant|actor)\s+(.+?)(?:@\{[^}]*\})?(?:\s+as\s+(.+))?$', st)
        if m:
            seq_part(sd, m.group(2), decode(m.group(3)) if m.group(3) else None, m.group(1))
            continue
        m = re.match(r'^create\s+(participant|actor)\s+(.+?)(?:\s+as\s+(.+))?$', st)

        if m:
            seq_part(sd, m.group(2), decode(m.group(3)) if m.group(3) else None, m.group(1))
            sd.simplified.add('create')
            continue
        m = re.match(r'^destroy\s+(\S+)$', st)
        if m:
            sd.events.append(('destroy', seq_part(sd, m.group(1))))
            continue
        m = re.match(r'^(activate|deactivate)\s+(\S+)$', st)
        if m:
            sd.events.append((m.group(1), seq_part(sd, m.group(2))))
            continue
        m = re.match(r'^autonumber(?:\s+(\d+))?(?:\s+(\d+))?$', st)
        if m:
            sd.autonumber = (int(m.group(1) or 1), int(m.group(2) or 1))
            continue
        m = re.match(r'^note\s+(left of|right of|over)\s+([^:]+?)\s*:\s*(.*)$', st, re.I)
        if m:
            who = [seq_part(sd, x) for x in m.group(2).split(',')]
            sd.events.append(('note', m.group(1).lower(), who, decode(m.group(3))))
            continue
        m = re.match(r'^(loop|alt|opt|par|critical|break|rect)\b\s*(.*)$', st)
        if m:
            label = decode(m.group(2)) if m.group(1) != 'rect' else ''
            sd.events.append(('open', m.group(1), label))
            stack.append(m.group(1))
            continue
        m = re.match(r'^box\b', st)
        if m:
            stack.append('box')
            sd.simplified.add('box')
            continue
        m = re.match(r'^(else|and|option)\b\s*(.*)$', st)
        if m:
            if not stack or stack[-1] == 'box':
                raise Unsupported('"%s" outside a block' % m.group(1))
            sd.events.append(('divider', m.group(1), decode(m.group(2))))
            continue
        if st == 'end':
            if not stack:
                raise Unsupported('"end" without a block')
            if stack.pop() != 'box':
                sd.events.append(('close',))
            continue
        m = re.match(r'^links?\s+\S+\s*:', st)
        if m:
            continue
        m = MSG.match(st)
        if m:
            a = seq_part(sd, m.group('a'))
            b = seq_part(sd, m.group('b'))
            pass
            sd.events.append(('msg', a, b, m.group('arrow'), m.group('act'), decode(m.group('text'))))
            continue
        raise Unsupported('unexpected statement "%s"' % st[:20])
    if stack:
        raise Unsupported('block without "end"')
    if not sd.parts:
        raise Unsupported('no participants')
    return sd


def seq_draw(sd, label_w, gap, spacer, strict_titles=True):
    n = len(sd.parts)
    boxes = []
    for p in sd.parts:
        lines = wrap_label(p['label'], label_w)
        lw = max(width(l) for l in lines)
        inner = lw + 2
        if inner % 2 == 0:
            inner += 1
        boxes.append({'lines': lines, 'w': inner + 2, 'h': len(lines) + 2})
    half = [(b['w'] - 1) // 2 for b in boxes]
    cons = defaultdict(int)   # (i, j) -> min distance c_j - c_i
    left_need = [0] * n       # how far left of c_i things reach (for i == 0 only matters)
    right_need = [0] * n
    for i in range(n):
        left_need[i] = max(left_need[i], half[i])
        right_need[i] = max(right_need[i], half[i])
    for i in range(n - 1):
        cons[(i, i + 1)] = max(cons[(i, i + 1)], half[i] + half[i + 1] + gap + 1)

    def need(i, j, d):
        if i > j:
            i, j = j, i
        if i == j:
            return
        cons[(i, j)] = max(cons[(i, j)], d)
    # frame structure
    frames = []
    stackf = []
    num = sd.autonumber[0] if sd.autonumber else None
    evs = []
    for ev in sd.events:
        if ev[0] == 'msg':
            text = ev[5]
            if num is not None:
                text = '%d. %s' % (num, text)
                num += sd.autonumber[1]
            ev = ev[:5] + (text,)
        evs.append(ev)
        if ev[0] == 'open':
            fr = {'kind': ev[1], 'label': ev[2], 'parts': set(), 'children': [], 'dividers': []}
            if stackf:
                stackf[-1]['children'].append(fr)
            stackf.append(fr)
            frames.append(fr)
        elif ev[0] == 'close':
            fr = stackf.pop()
            if stackf:
                stackf[-1]['parts'] |= fr['parts']
        elif ev[0] == 'divider':
            stackf[-1]['dividers'].append(ev)
        else:
            touched = set()
            if ev[0] == 'msg':
                touched = {ev[1], ev[2]}
            elif ev[0] == 'note':
                touched = set(ev[2])
            elif ev[0] in ('activate', 'deactivate', 'destroy'):
                touched = {ev[1]}
            for fr in stackf:
                fr['parts'] |= touched

    def depth(fr):
        return 1 + max((depth(c) for c in fr['children']), default=0)
    for fr in frames:
        if not fr['parts']:
            fr['parts'] = {0}
        fr['i'], fr['j'] = min(fr['parts']), max(fr['parts'])
        fr['m'] = 2 * depth(fr)
        titles = [fr['kind'] + (' [%s]' % fr['label'].replace('\n', ' ') if fr['label'] else '')]
        titles += [d[1] + (' [%s]' % d[2].replace('\n', ' ') if d[2] else '') for d in fr['dividers']]
        fr['tw'] = max(width(t) for t in titles)
        i, j, m = fr['i'], fr['j'], fr['m']
        # title starts at c_i + 2 and needs room before the right border
        if i < j:
            need(i, j, fr['tw'] + 4 - m)
            if strict_titles:
                need(i, i + 1, fr['tw'] + 5)
            fr['ext'] = m
        else:
            fr['ext'] = max(m, fr['tw'] + 4)
        if i > 0:
            need(i - 1, i, m + 1)
        else:
            left_need[0] = max(left_need[0], m)
        if j < n - 1:
            need(j, j + 1, fr['ext'] + 1)
        else:
            right_need[j] = max(right_need[j], fr['ext'])
    lab = {}
    for idx, ev in enumerate(evs):
        if ev[0] == 'msg':
            a, b, text = ev[1], ev[2], ev[5]
            lines = wrap_label(text, label_w) if text else ['']
            L = max(width(l) for l in lines)
            lab[idx] = lines
            if a == b:
                if a < n - 1:
                    need(a, a + 1, L + 6)
                else:
                    right_need[a] = max(right_need[a], 4 + L)
            else:
                need(a, b, max(L + 3, 4))
        elif ev[0] == 'note':
            lines = wrap_label(ev[3], label_w)
            L = max(width(l) for l in lines)
            nw = L + 4
            lab[idx] = lines
            where, who = ev[1], ev[2]
            if where == 'right of':
                i = who[0]
                if i < n - 1:
                    need(i, i + 1, nw + 3)
                else:
                    right_need[i] = max(right_need[i], nw + 2)
            elif where == 'left of':
                i = who[0]
                if i > 0:
                    need(i - 1, i, nw + 3)
                else:
                    left_need[0] = max(left_need[0], nw + 2)
            else:
                i, j = min(who), max(who)
                if i == j:
                    lo_, hi_ = (nw - 1) // 2, nw - 1 - (nw - 1) // 2
                else:
                    need(i, j, nw - 5)
                    lo_ = hi_ = 2
                if i > 0:
                    need(i - 1, i, lo_ + 2)
                else:
                    left_need[0] = max(left_need[0], lo_)
                if j < n - 1:
                    need(j, j + 1, hi_ + 2)
                else:
                    right_need[j] = max(right_need[j], hi_)
    c = [0] * n
    c[0] = left_need[0]
    for j in range(1, n):
        c[j] = max(c[i] + d for (i, jj), d in cons.items() if jj == j)
    width_total = max(c[i] + right_need[i] for i in range(n)) + 1
    cv = Canvas()
    hh = max(b['h'] for b in boxes)
    for i, b in enumerate(boxes):
        x0 = c[i] - half[i]
        paint_box(cv, x0, 0, {'w': b['w'], 'h': b['h'], 'shape': 'rect', 'lines': b['lines'], 'slant': None})
    y = hh
    if spacer:
        y += 1
    blocked = defaultdict(list)   # participant -> list of (y0, y1) rows where the lifeline is hidden
    active = defaultdict(list)    # participant -> list of [start, end]
    act_depth = defaultdict(int)
    dead = {}
    open_frames = []
    fr_iter = iter(frames)
    for idx, ev in enumerate(evs):
        kind = ev[0]
        if kind == 'msg':
            a, b, arrow, act = ev[1], ev[2], ev[3], ev[4]
            lines = lab[idx]
            dotted = arrow.startswith('--') or arrow.startswith('<<--')
            stroke = 'dotted' if dotted else 'solid'
            if a == b:
                rows = max(2, len(lines))
                x = c[a]
                cv.line(y, x, E)
                cv.line(y, x + 1, W | E, stroke)
                cv.line(y, x + 2, W | S, stroke)
                for yy in range(y + 1, y + rows - 1):
                    cv.line(yy, x + 2, N | S, stroke)
                cv.line(y + rows - 1, x + 2, N | W, stroke)
                cv.glyph(y + rows - 1, x + 1, '◂')
                for i, l in enumerate(lines):
                    cv.put_text(y + i, x + 4, l)
                y += rows
            else:
                lft = min(c[a], c[b])
                for i, l in enumerate(lines):
                    cv.put_text(y + i, lft + 2, l, over_line=abs(a - b) > 1)

                y += len(lines) if lines != [''] else 0
                xa, xb = c[a], c[b]
                dirn = 1 if xb > xa else -1
                both = arrow.startswith('<<')
                head = {'->>': 'arrow', '-->>': 'arrow', '->': 'none', '-->': 'none', '-x': 'cross',
                        '--x': 'cross', '-)': 'open', '--)': 'open'}.get(arrow, 'arrow')
                start = xa + dirn
                end = xb - dirn
                if not both:
                    cv.line(y, xa, E if dirn > 0 else W)
                if head == 'none':
                    seq_paint(cv, [(y, start if not both else start + dirn), (y, xb)], stroke)
                else:
                    seq_paint(cv, [(y, start if not both else start + dirn), (y, end - dirn)], stroke)
                    hg = {'arrow': '▸' if dirn > 0 else '◂', 'open': '▹' if dirn > 0 else '◃',
                          'cross': '✕'}[head]
                    cv.glyph(y, end, hg)
                if both:
                    cv.glyph(y, start, '◂' if dirn > 0 else '▸')
                y += 1
                if act == '+':
                    active[b].append([y, None])
                elif act == '-':
                    for span_ in reversed(active[a]):
                        if span_[1] is None:
                            span_[1] = y
                            break
            if spacer:
                y += 1
        elif kind == 'note':
            lines = lab[idx]
            L = max(width(l) for l in lines)
            nw = L + 4
            where, who = ev[1], ev[2]
            if where == 'right of':
                x0 = c[who[0]] + 2
            elif where == 'left of':
                x0 = c[who[0]] - 2 - nw + 1
            else:
                i, j = min(who), max(who)
                if i == j:
                    x0 = c[i] - (nw - 1) // 2
                else:
                    x0 = c[i] - 2
                    nw = max(nw, c[j] + 2 - x0 + 1)
            h = len(lines) + 2
            paint_box(cv, x0, y, {'w': nw, 'h': h, 'shape': 'rect', 'lines': lines, 'slant': None})
            for k in range(n):
                if x0 < c[k] < x0 + nw - 1:
                    blocked[k].append((y, y + h - 1))
            y += h
            if spacer:
                y += 1
        elif kind == 'open':
            fr = next(fr_iter)
            fr['y0'] = y
            open_frames.append(fr)
            y += 1
        elif kind == 'divider':
            fr = open_frames[-1]
            fr.setdefault('divs', []).append((y, ev))
            y += 1
        elif kind == 'close':
            fr = open_frames.pop()
            fr['y1'] = y
            y += 1
        elif kind == 'activate':
            active[ev[1]].append([y, None])
        elif kind == 'deactivate':
            for span_ in reversed(active[ev[1]]):
                if span_[1] is None:
                    span_[1] = y
                    break
        elif kind == 'destroy':
            cv.glyph(y, c[ev[1]], '✕')
            dead[ev[1]] = y
            y += 1
    last = y
    # frames
    for fr in frames:
        x0 = c[fr['i']] - fr['m']
        x1 = c[fr['j']] + fr['ext']
        y0, y1 = fr['y0'], fr['y1']
        seq_paint(cv, [(y0, x1), (y0, x0), (y1, x0), (y1, x1), (y0, x1)], 'solid')
        title = fr['kind'] + (' [%s]' % fr['label'].replace('\n', ' ') if fr['label'] else '')
        cv.put_text(y0, c[fr['i']] + 2, ' ' + title + ' ', over_line=True)
        for (yd, ev) in fr.get('divs', []):
            seq_paint(cv, [(yd, x0), (yd, x1)], 'dotted')
            t = ev[1] + (' [%s]' % ev[2].replace('\n', ' ') if ev[2] else '')
            cv.put_text(yd, c[fr['i']] + 2, ' ' + t + ' ', over_line=True)
    # lifelines
    for k in range(n):
        x = c[k]
        top = boxes[k]['h'] - 1
        bottom = dead.get(k, last) - (1 if k in dead else 0)
        cuts = sorted(blocked[k])
        segs = []
        cur = top
        for (b0, b1) in cuts:
            segs.append((cur, b0))
            cur = b1
        segs.append((cur, bottom))
        for (s0, s1) in segs:
            if s1 > s0:
                for yy in range(s0, s1 + 1):
                    bits = (S if yy < s1 else 0) | (N if yy > s0 else 0)
                    heavy = any(sp[0] <= yy < (sp[1] if sp[1] is not None else last) for sp in active[k])
                    cv.line(yy, x, bits, 'thick' if heavy and s0 < yy < s1 else 'solid')
    return cv.render(), cv.violations


def seq_transcript(sd, w):
    rows = []
    num = 1
    depth = 0
    for ev in sd.events:
        ind = '  ' * depth
        if ev[0] == 'msg':
            a, b = sd.parts[ev[1]]['label'], sd.parts[ev[2]]['label']
            arrow = '┄▸' if ev[3].startswith('--') else '─▸'
            line = '%s%d. %s %s %s: %s' % (ind, num, a.replace('\n', ' '), arrow, b.replace('\n', ' '), ev[5].replace('\n', ' '))
            num += 1
        elif ev[0] == 'note':
            line = '%sNote %s %s: %s' % (ind, ev[1], ', '.join(sd.parts[k]['label'] for k in ev[2]), ev[3].replace('\n', ' '))
        elif ev[0] == 'open':
            line = '%s%s %s' % (ind, ev[1], ev[2])
            depth += 1
        elif ev[0] == 'divider':
            ind = '  ' * (depth - 1)
            line = '%s %s' % (ev[1], ev[2])
        elif ev[0] == 'close':
            depth -= 1
            ind = '  ' * depth
            line = 'end'
        else:
            continue

        ws = wrap(line.strip(), w - len(ind))
        rows.append(ind + ws[0])
        for cnt in ws[1:]:
            rows.append(ind + '   ' + cnt)
    return rows


# ------------------------------------------------------------- ladder
def kind_of(src):
    title, body = preamble(src)
    st = statements(body)
    if not st:
        raise Unsupported('empty diagram')
    kw = st[0].split()[0]
    return title, kw, st


def render(src, width_, natural=False):
    """Return (outcome, rows, step, notes, kind). outcome: drawn, outline, source."""
    try:
        title, kw, st = kind_of(src)
    except Unsupported as e:
        return 'source', [], None, [str(e)], 'unknown'
    try:
        if kw in ('flowchart', 'graph', 'flowchart-v2', 'flowchart-elk'):
            g = parse_flow(st[0], st[1:])
            nn = len([n for n in g.nodes if not n.get('dead')])
            if nn > 150 or len(g.edges) > 300 or len(src) > 65536:
                return 'outline', flow_outline(g, width_), 'too large', ['too large'], 'flowchart'
            prep = flow_prepare(g)
            d = 'LR' if g.dir in ('LR', 'RL') else 'TD'
            ladder = [Prof('normal', 28, 3, 4, d), Prof('compact', 28, 1, 2, d)]
            if d == 'LR':
                ladder += [Prof('turned', 28, 3, 4, 'TD'), Prof('turned compact', 28, 1, 2, 'TD')]
            ladder += [Prof('labels 16', 16, 1, 2, 'TD'), Prof('labels 8', 8, 1, 2, 'TD')]
            notes = sorted(g.simplified)
            for prof in ladder:
                rows, viol = flow_draw(prep, prof)
                wmax = max((width(r) for r in rows), default=0)
                if natural or wmax <= width_:
                    return 'drawn', rows, prof.name, notes + (['violations %d' % viol] if viol else []), 'flowchart'
            return 'outline', flow_outline(g, width_), 'outline', notes, 'flowchart'
        if kw == 'sequenceDiagram':
            sd = parse_seq(st[0], st[1:])
            if len(sd.parts) > 40 or len(sd.events) > 500:
                return 'outline', seq_transcript(sd, width_), 'too large', ['too large'], 'sequence'
            ladder = [('normal', 28, 3, True), ('compact', 28, 1, False), ('labels 16', 16, 1, False),
                      ('labels 8', 8, 1, False)]
            for name, lw, gap, spacer in ladder:
                rows, viol = seq_draw(sd, lw, gap, spacer, name in ('normal', 'compact'))

                wmax = max((width(r) for r in rows), default=0)
                if natural or wmax <= width_:
                    return 'drawn', rows, name, sorted(sd.simplified) + (['violations %d' % viol] if viol else []), 'sequence'
            return 'outline', seq_transcript(sd, width_), 'outline', sorted(sd.simplified), 'sequence'
        return 'source', [], None, ['%s diagrams are not drawn' % kw], 'other'
    except Unsupported as e:
        return 'source', [], None, [str(e)], 'flowchart' if kw in ('flowchart', 'graph') else ('sequence' if kw == 'sequenceDiagram' else 'other')


def framed(src, width_, kind_label=None):
    outcome, rows, step, notes, kind = render(src, width_)
    label = ' mermaid · %s%s ' % (kind, ' (outline)' if outcome == 'outline' else '')
    top = '──' + label + '─' * max(0, width_ - 2 - width(label))
    note = ''
    if step and step.startswith('turned'):
        note = ' drawn top-down to fit '
    if outcome == 'outline':
        note = ' Enter: full drawing '
    bottom = '─' * width_ if not note else '─' * (width_ - width(note) - 2) + note + '──'
    out = [top]
    dw = max((width(r) for r in rows), default=0)
    pad = (width_ - dw) // 2 if outcome == 'drawn' else 0
    for r in rows:
        out.append((' ' * pad + r).rstrip())
    out.append(bottom)
    return out, outcome, step, notes


if __name__ == '__main__':
    src = sys.stdin.read()
    w = int(sys.argv[1]) if len(sys.argv) > 1 else 80
    rows, outcome, step, notes = framed(src, w)
    print('\n'.join(rows))
    print('[%s, %s, %s]' % (outcome, step, notes))
