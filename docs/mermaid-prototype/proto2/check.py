"""Correctness checks on a painted prototype canvas.

flow_check: trace lines on the visible canvas, cell to cell, and compare what
each connected net joins with the edge set. A net that contains edges with
sources S and targets T tells the reader that every s in S reaches every t in
T; any such pair that is not an edge (in either direction) is a false
connection.

cell_check: cells shared by two different buses (different bus and different
edge) are either a clean perpendicular crossing or a bad overlap.

seq_check: a message must not form a junction with any lifeline or frame
between its two ends.
"""
from collections import defaultdict

import proto as P

N, S, W, E = P.N, P.S, P.W, P.E


def cell_check(cv):
    crossings = bad = 0
    for (y, x), d in cv.own.items():
        if len(d) < 2:
            continue
        gs = cv.groups(y, x)
        if len(gs) < 2:
            continue
        bits = []
        for g in gs:
            b = 0
            for j in g:
                b |= j[2]
            bits.append(b)
        if len(bits) == 2 and sorted(bits) == sorted([N | S, W | E]):
            crossings += 1
        else:
            bad += 1
    return crossings, bad


def flow_check(cv, edges):
    """edges: {k: (a, b)} for visible edges. Returns the number of false pairs."""
    owned = [c for c, d in cv.own.items() if d]
    if not owned:
        return 0
    vis = {c: cv.vis(*c) for c in owned}
    idx = {c: i for i, c in enumerate(owned)}
    par = list(range(len(owned)))

    def f(i):
        while par[i] != i:
            par[i] = par[par[i]]
            i = par[i]
        return i
    for c in owned:
        y, x = c
        b = vis[c]
        for bit, dy, dx, opp in ((E, 0, 1, W), (S, 1, 0, N)):
            if b & bit:
                nb = (y + dy, x + dx)
                if nb in idx and vis[nb] & opp:
                    par[f(idx[c])] = f(idx[nb])
    nets = defaultdict(set)
    for c in owned:
        root = f(idx[c])
        for (bus, k), bb in cv.own[c].items():
            if k is not None and k in edges and bb & vis[c]:
                nets[root].add(k)
    eset = set(edges.values())
    false_pairs = 0
    for ks in nets.values():
        srcs = {edges[k][0] for k in ks}
        dsts = {edges[k][1] for k in ks}
        for s in srcs:
            for t in dsts:
                if s != t and (s, t) not in eset and (t, s) not in eset:
                    false_pairs += 1
    return false_pairs


def seq_check(cv):
    """Returns (messages that form a junction between their ends, frame/lifeline junction cells)."""
    bad_msgs = 0
    for (y, x0, x1) in cv.msgs:
        for x in range(x0 + 1, x1):
            if cv.vis(y, x) & (N | S):
                bad_msgs += 1
                break
    frame_j = 0
    for (y, x), d in cv.own.items():
        kinds = {bus[0] for (bus, k) in d}
        if 'frame' in kinds and 'life' in kinds:
            v = cv.vis(y, x)
            if v & (N | S) and v & (W | E):
                frame_j += 1
    return bad_msgs, frame_j
