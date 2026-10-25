"""Build proto2/proto.py from proto/proto.py with checks and the two routing fixes.

Every replacement must match exactly once. Nothing in the folio repository is touched.
"""
import sys

src_path, dst_path = sys.argv[1], sys.argv[2]
src = open(src_path, encoding='utf-8').read()

R = []


def rep(old, new):
    R.append((old, new))


# module switches
rep("from collections import defaultdict, deque\n",
    "from collections import defaultdict, deque\n\n"
    "FIX = False        # crossing glyph rule + separate ports for different buses\n"
    "CJK_BREAK = False  # allow label breaks between wide characters\n"
    "LAST = {}\n")

# canvas: owners per cell, crossing mask
rep("        self.intended = set()\n        self.violations = 0\n",
    "        self.intended = set()\n        self.violations = 0\n"
    "        self.owner = None\n"
    "        self.own = defaultdict(dict)\n"
    "        self.fix = False\n"
    "        self.front = lambda bus, b: 1\n"
    "        self.hard_cycles = 0\n"
    "        self.dropped = 0\n"
    "        self.msgs = []\n"
    "        self.odot = defaultdict(dict)\n")

rep("        if self.dotted.get((y, x)) and not self.solid.get((y, x)):\n",
    "        dashed = self.dotted.get((y, x)) and not self.solid.get((y, x))\n"
    "        if self.fix:\n"
    "            fd = self.front_dotted(y, x)\n"
    "            if fd is not None:\n"
    "                dashed = fd\n"
    "        if dashed:\n")

rep("            self.solid[(y, x)] += 1\n\n    def glyph(self, y, x, ch):\n",
    "            self.solid[(y, x)] += 1\n"
    "        if self.owner is not None:\n"
    "            d = self.own[(y, x)]\n"
    "            d[self.owner] = d.get(self.owner, 0) | bits\n"
    "            self.odot[(y, x)][self.owner] = stroke == 'dotted'\n\n"
    "    def front_dotted(self, y, x):\n"
    "        \"\"\"At a crossing, whether the line in front is dotted (None if not a crossing).\"\"\"\n"
    "        gs = self.groups(y, x)\n"
    "        if len(gs) < 2:\n"
    "            return None\n"
    "        best, dotted = None, None\n"
    "        for g in gs:\n"
    "            b = 0\n"
    "            for j in g:\n"
    "                b |= j[2]\n"
    "            p = max(self.front(j[0], b) for j in g)\n"
    "            if best is None or p > best:\n"
    "                best = p\n"
    "                dotted = all(self.odot[(y, x)].get((j[0], j[1]), False) for j in g)\n"
    "        return dotted\n\n"
    "    def groups(self, y, x):\n"
    "        \"\"\"Owners in a cell, merged when they share a bus or an edge.\"\"\"\n"
    "        d = self.own.get((y, x))\n"
    "        if not d:\n"
    "            return []\n"
    "        gs = []\n"
    "        for (bus, k), b in d.items():\n"
    "            it = (bus, k, b)\n"
    "            merged = [g for g in gs if any(bus == j[0] or (k is not None and k == j[1]) for j in g)]\n"
    "            gs = [g for g in gs if not any(g is m for m in merged)] + [[it] + [j for g in merged for j in g]]\n"
    "        return gs\n\n"
    "    def mask(self, y, x):\n"
    "        gs = self.groups(y, x)\n"
    "        if len(gs) < 2:\n"
    "            return 0xF\n"
    "        info = []\n"
    "        for g in gs:\n"
    "            b = 0\n"
    "            for j in g:\n"
    "                b |= j[2]\n"
    "            info.append((max(self.front(j[0], b) for j in g), b))\n"
    "        top = max(p for p, b in info)\n"
    "        front = back = 0\n"
    "        for p, b in info:\n"
    "            if p == top:\n"
    "                front |= b\n"
    "            else:\n"
    "                back |= b\n"
    "        return 0xF & ~(back & ~front)\n\n"
    "    def vis(self, y, x):\n"
    "        b = self.light.get((y, x), 0) | self.heavy.get((y, x), 0)\n"
    "        if self.fix:\n"
    "            b &= self.mask(y, x)\n"
    "        return b\n\n"
    "    def glyph(self, y, x, ch):\n")

rep("        lb, hb = self.light.get((y, x), 0), self.heavy.get((y, x), 0)\n        bits = lb | hb\n",
    "        lb, hb = self.light.get((y, x), 0), self.heavy.get((y, x), 0)\n"
    "        if self.fix:\n"
    "            m = self.mask(y, x)\n"
    "            lb, hb = lb & m, hb & m\n"
    "        bits = lb | hb\n")

# flowchart drawing
rep("    TD = prof.dir == 'TD'\n    cv = Canvas()\n",
    "    TD = prof.dir == 'TD'\n    cv = Canvas()\n"
    "    cv.fix = FIX\n"
    "    flow_bits = (N | S) if TD else (W | E)\n"
    "    cv.front = lambda bus, b: 1 if (b & ~flow_bits) == 0 else 0\n")

rep("    off = {}\n    P = {e: i for r in range(R + 1) for i, e in enumerate(order[r])}\n",
    "    off = {}\n    P = {e: i for r in range(R + 1) for i, e in enumerate(order[r])}\n"
    "    fanout = {}\n"
    "    for x0_ in list(down):\n"
    "        if elems[x0_]['kind'] != 'node':\n"
    "            continue\n"
    "        cnt_ = defaultdict(int)\n"
    "        for z in down[x0_]:\n"
    "            k_ = link_edge[(x0_, z)]\n"
    "            cnt_[(ED[k_]['stroke'], ends(k_)[0])] += 1\n"
    "        for z in down[x0_]:\n"
    "            k_ = link_edge[(x0_, z)]\n"
    "            fanout[(x0_, z)] = cnt_[(ED[k_]['stroke'], ends(k_)[0])]\n")

rep("                key = (ED[k]['stroke'], hu if side == 'far' else hl)\n",
    "                key = (ED[k]['stroke'], hu if side == 'far' else hl)\n"
    "                if FIX and side == 'near' and k in chains:\n"
    "                    ch_ = chains[k]\n"
    "                    j_ = ch_.index(y)\n"
    "                    while j_ > 0 and elems[ch_[j_]]['kind'] == 'dummy':\n"
    "                        j_ -= 1\n"
    "                    if elems[ch_[j_]]['kind'] == 'node' and fanout.get((ch_[j_], ch_[j_ + 1]), 1) > 1:\n"
    "                        key = key + (ch_[j_],)\n")

rep("        ns = [s for s in seginfo if not s['straight']]\n",
    "        ns = [s for s in seginfo if not s['straight']]\n"
    "        for si_, s_ in enumerate(seginfo):\n"
    "            for p_ in s_['ps']:\n"
    "                p_['bus'] = ('bus', r, si_)\n")

rep("                hab, hba = bool(a['tops'] & b['bots']), bool(b['tops'] & a['bots'])\n",
    "                hab, hba = bool(a['tops'] & b['bots']), bool(b['tops'] & a['bots'])\n"
    "                if hab and hba:\n"
    "                    cv.hard_cycles += 1\n")

rep("            above[a].pop(b, None)\n",
    "            if above[a].get(b, 0) >= INF:\n"
    "                cv.hard_cycles += 1\n"
    "            above[a].pop(b, None)\n")

rep("        for p in pieces:\n            e1, e2 = p['e1'], p['e2']\n",
    "        for p in pieces:\n            e1, e2 = p['e1'], p['e2']\n"
    "            cv.owner = (p['bus'], p['k'])\n")

rep("            apath([(RS[r], pos[e]), (RS[r] + thick[r] - 1, pos[e])], ED[el['edge']]['stroke'], (-1, 0), (1, 0))\n",
    "            cv.owner = (('dummy', e), el['edge'])\n"
    "            apath([(RS[r], pos[e]), (RS[r] + thick[r] - 1, pos[e])], ED[el['edge']]['stroke'], (-1, 0), (1, 0))\n")

rep("    # nodes\n    rects = {}\n", "    # nodes\n    cv.owner = None\n    rects = {}\n")

rep("            g.simplified.add('LR self-loop')\n",
    "            g.simplified.add('LR self-loop')\n            cv.dropped += 1\n")

rep("        x0, y0, w, h = rects[k]\n        xr = x0 + w - 1\n",
    "        x0, y0, w, h = rects[k]\n        xr = x0 + w - 1\n"
    "        cv.owner = (('self', ks[0]), ks[0])\n")

rep("        stroke = ED[k]['stroke']\n        head = ED[k]['hb']\n        cells = cv.cells()\n",
    "        stroke = ED[k]['stroke']\n        head = ED[k]['hb']\n        cells = cv.cells()\n"
    "        cv.owner = (('lane', u), k)\n")

rep("    rows = cv.render()\n    return rows, cv.violations\n",
    "    cv.owner = None\n    rows = cv.render()\n    LAST['cv'] = cv\n    return rows, cv.violations\n")

# sequence drawing
rep("    width_total = max(c[i] + right_need[i] for i in range(n)) + 1\n    cv = Canvas()\n",
    "    width_total = max(c[i] + right_need[i] for i in range(n)) + 1\n    cv = Canvas()\n"
    "    cv.fix = FIX\n"
    "    cv.front = lambda bus, b: 0 if bus[0] == 'life' else 1\n")

rep("    for idx, ev in enumerate(evs):\n        kind = ev[0]\n",
    "    for idx, ev in enumerate(evs):\n        kind = ev[0]\n        cv.owner = None\n")

rep("                cv.line(y, x, E)\n                cv.line(y, x + 1, W | E, stroke)\n",
    "                cv.line(y, x, E)\n"
    "                cv.owner = (('msg', idx), idx)\n"
    "                cv.line(y, x + 1, W | E, stroke)\n")

rep("                if head == 'none':\n                    seq_paint(cv, [(y, start if not both else start + dirn), (y, xb)], stroke)\n",
    "                cv.owner = (('msg', idx), idx)\n"
    "                cv.msgs.append((y, min(xa, xb), max(xa, xb)))\n"
    "                if head == 'none':\n"
    "                    seq_paint(cv, [(y, start if not both else start + dirn), (y, xb)], stroke)\n"
    "                    cv.own[(y, xb)].pop(cv.owner, None)\n")

rep("    for fr in frames:\n        x0 = c[fr['i']] - fr['m']\n",
    "    for fr in frames:\n        x0 = c[fr['i']] - fr['m']\n"
    "        cv.owner = (('frame', id(fr)), None)\n")

rep("    for k in range(n):\n        x = c[k]\n        top = boxes[k]['h'] - 1\n",
    "    for k in range(n):\n        x = c[k]\n        top = boxes[k]['h'] - 1\n"
    "        cv.owner = (('life', k), None)\n")

rep("    return cv.render(), cv.violations\n",
    "    cv.owner = None\n    rows = cv.render()\n    LAST['cv'] = cv\n    return rows, cv.violations\n")

# labels: optional breaks between wide characters
rep("def wrap_label(text, w):\n    \"\"\"Wrap a label; words up to 16 columns are never broken.\"\"\"\n"
    "    longest = max((width(x) for x in text.split()), default=0)\n",
    "def wrap_label(text, w):\n    \"\"\"Wrap a label; words up to 16 columns are never broken.\"\"\"\n"
    "    longest = max((width(x) for x in text.split()), default=0)\n"
    "    if CJK_BREAK:\n"
    "        longest = 0\n"
    "        for word in text.split():\n"
    "            run = 0\n"
    "            for g_ in graphemes(word):\n"
    "                if width(g_) >= 2:\n"
    "                    longest = max(longest, run, width(g_))\n"
    "                    run = 0\n"
    "                else:\n"
    "                    run += width(g_)\n"
    "            longest = max(longest, run)\n")

for old, new in R:
    n = src.count(old)
    if n != 1:
        sys.exit('pattern found %d times: %r' % (n, old[:80]))
    src = src.replace(old, new)
open(dst_path, 'w', encoding='utf-8').write(src)
print('ok, %d replacements' % len(R))
