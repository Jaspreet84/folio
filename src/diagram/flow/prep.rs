//! Steps that run once per diagram (plan section 6.1): cycles, ranks,
//! chains, order and lanes, then the lines drawn on each link. Ported from
//! `flow_prepare` in the prototype, except for the lines: the prototype
//! drew only the last edge of a link.
//!
//! Every list is in a fixed order and ties are broken by declaration
//! order; the hash map is only for lookups.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use super::{FlowGraph, Head, Stroke};
use crate::diagram::{NotDrawn, Work};

/// After preparation, a rank may hold at most this many elements
/// (plan section 6.6).
pub const MAX_RANK_ELEMS: usize = 40;
/// After preparation, the diagram may hold at most this many elements.
pub const MAX_ELEMS: usize = 300;

/// Elements built before lanes are chosen, and ranks. Far above anything
/// that fits [`MAX_ELEMS`] after the lanes' dummies leave; it only stops a
/// huge minimum length from allocating without bound.
const MAX_BUILT: usize = 50 * MAX_ELEMS;

/// Barycenter sweeps, and sweeps without improvement before stopping.
const SWEEPS: usize = 8;
const STALE_SWEEPS: usize = 4;
/// Passes of adjacent swaps after the sweeps.
const TRANSPOSE_PASSES: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElemKind {
    Node,
    /// An edge label: the first element after an edge's upper end.
    Label,
    /// A point an edge passes through on a rank between its ends.
    Dummy,
}

/// One thing placed on a rank.
#[derive(Clone, Copy, Debug)]
pub struct Elem {
    pub kind: ElemKind,
    pub rank: usize,
    /// The node, for [`ElemKind::Node`].
    pub node: usize,
    /// The edge, for a label or dummy.
    pub edge: usize,
}

/// Everything [`super::draw::draw`] needs that doesn't depend on the
/// ladder step. Boxed, so the diagram cache's entries stay small.
#[derive(Clone, Debug, Default)]
pub struct Prepared(Box<Parts>);

impl std::ops::Deref for Prepared {
    type Target = Parts;

    fn deref(&self) -> &Parts {
        &self.0
    }
}

/// The contents of [`Prepared`].
#[derive(Clone, Debug, Default)]
pub struct Parts {
    /// Each node's rank (0 for nodes that aren't drawn).
    pub rank: Vec<usize>,
    /// The highest rank.
    pub max_rank: usize,
    pub elems: Vec<Elem>,
    /// The elements of each rank, in order across the flow.
    pub order: Vec<Vec<usize>>,
    /// By edge: its elements from upper end to lower end. `None` for
    /// invisible edges, self-loops and lanes.
    pub chains: Vec<Option<Vec<usize>>>,
    /// By edge: (upper node, lower node, reversed), for every edge
    /// between two different nodes.
    pub info: Vec<Option<(usize, usize, bool)>>,
    /// By element: the elements linked to it in the rank above and below,
    /// once per edge, so parallel edges repeat a neighbour (as in the
    /// prototype, for ordering).
    pub up: Vec<Vec<usize>>,
    pub down: Vec<Vec<usize>>,
    /// The lines drawn on each link (upper, lower). Only a link between two
    /// nodes can carry more than one edge: edges joining the same two nodes
    /// on neighbouring ranks, in either direction.
    pub link_lines: HashMap<(usize, usize), Vec<Line>>,
    /// By element: (neighbour, line on that link) for each line to the rank
    /// below and above, in the order of `down` and `up`.
    pub down_lines: Vec<Vec<(usize, usize)>>,
    pub up_lines: Vec<Vec<(usize, usize)>>,
    /// Back edges drawn in their own lane beside the drawing.
    pub lanes: Vec<usize>,
    /// Visible edges from a node to itself.
    pub selfloops: Vec<usize>,
    /// (node, element) for each drawn node, in declaration order.
    pub node_elem: Vec<(usize, usize)>,
    /// By node: its element.
    pub elem_of: Vec<Option<usize>>,
    /// By edge: reversed to break a cycle.
    pub back: Vec<bool>,
}

impl Parts {
    /// The heads at the upper and lower end of edge `k`'s chain.
    pub fn ends(&self, g: &FlowGraph, k: usize) -> (Head, Head) {
        let e = &g.edges[k];
        match self.info[k] {
            Some((_, _, true)) => (e.head_to, e.head_from),
            _ => (e.head_from, e.head_to),
        }
    }

    /// Line `li` of the link from `a` down to `b`.
    pub fn line(&self, a: usize, b: usize, li: usize) -> Option<&Line> {
        self.link_lines.get(&(a, b)).and_then(|ls| ls.get(li))
    }
}

/// One line drawn on a link: the edges it stands for, its stroke and the
/// heads at its upper and lower ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// The edges it shows, in declaration order. The first owns its cells.
    pub edges: Vec<usize>,
    pub stroke: Stroke,
    pub upper: Head,
    pub lower: Head,
}

impl Line {
    /// The edge that owns the line's cells.
    pub fn owner(&self) -> usize {
        self.edges.first().copied().unwrap_or(0)
    }

    /// Whether the line has a head at both ends. Such a line leads into
    /// both of its nodes, so it never shares a port: through a shared port,
    /// it would seem to lead from every other line there to its far node.
    pub fn both_ways(&self) -> bool {
        self.upper != Head::None && self.lower != Head::None
    }
}

/// The head one end of a line shows for two edges, if one head can show
/// both: no head joins any head, and a head joins itself.
pub(super) fn join_heads(a: Head, b: Head) -> Option<Head> {
    match (a, b) {
        (Head::None, h) | (h, Head::None) => Some(h),
        _ => (a == b).then_some(a),
    }
}

/// Split the edges of one link into lines. Edges with the same stroke share
/// a line whose ends carry every head they ask for (`A-->B` and `B-->A`
/// give `◂──▸`). An edge that a line can't show truthfully, with another
/// stroke or another head at the same end, gets a line of its own beside it.
fn split_lines(parts: &Parts, g: &FlowGraph, ks: &[usize]) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::with_capacity(1);
    for &k in ks {
        let (upper, lower) = parts.ends(g, k);
        let stroke = g.edges[k].stroke;
        let fit = lines.iter_mut().find_map(|l| {
            let u = join_heads(l.upper, upper)?;
            let w = join_heads(l.lower, lower)?;
            (l.stroke == stroke).then_some((l, u, w))
        });
        match fit {
            Some((l, u, w)) => {
                l.edges.push(k);
                l.upper = u;
                l.lower = w;
            }
            None => lines.push(Line { edges: vec![k], stroke, upper, lower }),
        }
    }
    lines
}

/// Whether an edge has label text.
pub(crate) fn has_text(label: &Option<String>) -> bool {
    label.as_deref().is_some_and(|s| !s.is_empty())
}

pub fn prepare(g: &FlowGraph, work: &mut Work) -> Result<Prepared, NotDrawn> {
    let nn = g.nodes.len();
    let ne = g.edges.len();
    let alive = |v: usize| v < nn && !g.nodes[v].dead;
    let nodes: Vec<usize> = (0..nn).filter(|&v| alive(v)).collect();
    let usable = |k: usize| alive(g.edges[k].from) && alive(g.edges[k].to);
    let selfloops: Vec<usize> = (0..ne)
        .filter(|&k| usable(k) && g.edges[k].from == g.edges[k].to && g.edges[k].stroke != Stroke::Invisible)
        .collect();
    let real: Vec<usize> = (0..ne).filter(|&k| usable(k) && g.edges[k].from != g.edges[k].to).collect();
    let mut prep = Parts {
        rank: vec![0; nn],
        chains: vec![None; ne],
        info: vec![None; ne],
        elem_of: vec![None; nn],
        back: vec![false; ne],
        selfloops,
        ..Parts::default()
    };
    if nodes.is_empty() {
        prep.order = vec![Vec::new()];
        return Ok(Prepared(Box::new(prep)));
    }

    // B. Break cycles: depth-first from each node in declaration order.
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); nn];
    for &k in &real {
        out[g.edges[k].from].push(k);
    }
    let mut state = vec![0u8; nn];
    for &root in &nodes {
        if state[root] != 0 {
            continue;
        }
        state[root] = 1;
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(top) = stack.last_mut() {
            let u = top.0;
            let mut next = None;
            while top.1 < out[u].len() {
                let k = out[u][top.1];
                top.1 += 1;
                let v = g.edges[k].to;
                if state[v] == 1 {
                    prep.back[k] = true;
                } else if state[v] == 0 {
                    state[v] = 1;
                    next = Some(v);
                    break;
                }
            }
            match next {
                Some(v) => stack.push((v, 0)),
                None => {
                    state[u] = 2;
                    stack.pop();
                }
            }
        }
    }

    // C. Rank: longest path with label ranks, then pull sources down.
    let has_label = real.iter().any(|&k| g.edges[k].stroke != Stroke::Invisible && has_text(&g.edges[k].label));
    let f = if has_label { 2 } else { 1 };
    // (upper, lower, minimum length, edge, reversed)
    let mut dag: Vec<(usize, usize, i64, usize, bool)> = Vec::with_capacity(real.len());
    for &k in &real {
        let e = &g.edges[k];
        let rev = prep.back[k];
        let (u, v) = if rev { (e.to, e.from) } else { (e.from, e.to) };
        let ml = i64::try_from(e.min_len.max(1)).unwrap_or(i64::MAX / 4).min(1 << 40);
        dag.push((u, v, ml * f, k, rev));
    }
    let mut succ: Vec<Vec<(usize, i64)>> = vec![Vec::new(); nn];
    let mut indeg = vec![0usize; nn];
    for &(u, v, ml, _, _) in &dag {
        succ[u].push((v, ml));
        indeg[v] += 1;
    }
    let mut rank = vec![0i64; nn];
    let mut heap: BinaryHeap<Reverse<usize>> = nodes.iter().filter(|&&v| indeg[v] == 0).map(|&v| Reverse(v)).collect();
    let mut topo = Vec::with_capacity(nodes.len());
    let mut deg = indeg.clone();
    while let Some(Reverse(u)) = heap.pop() {
        topo.push(u);
        for &(v, ml) in &succ[u] {
            rank[v] = rank[v].max(rank[u] + ml);
            deg[v] -= 1;
            if deg[v] == 0 {
                heap.push(Reverse(v));
            }
        }
    }
    for &u in &topo {
        if indeg[u] == 0
            && let Some(r) = succ[u].iter().map(|&(v, ml)| rank[v] - ml).min()
        {
            rank[u] = r;
        }
    }
    let lo = nodes.iter().map(|&v| rank[v]).min().unwrap_or(0);
    for &v in &nodes {
        prep.rank[v] = usize::try_from(rank[v] - lo).unwrap_or(0);
    }
    let rank = &prep.rank;

    // D. Chains: each visible edge becomes node, label or dummies, node.
    // Count the elements first, so a huge minimum length fails early. Only
    // back edges' dummies can leave the order later (as lanes).
    let (mut count, mut kept) = (nodes.len(), nodes.len());
    for &(u, v, _, k, rev) in &dag {
        if g.edges[k].stroke != Stroke::Invisible {
            let d = rank[v].saturating_sub(rank[u]).saturating_sub(1);
            count = count.saturating_add(d);
            if !rev {
                kept = kept.saturating_add(d);
            }
        }
    }
    let max_rank = nodes.iter().map(|&v| rank[v]).max().unwrap_or(0);
    if kept > MAX_ELEMS || count > MAX_BUILT || max_rank > MAX_BUILT * 4 {
        return Err(NotDrawn::TooLarge);
    }
    work.spend(count as u64)?;
    let mut elems: Vec<Elem> = Vec::with_capacity(count);
    for &v in &nodes {
        prep.elem_of[v] = Some(elems.len());
        prep.node_elem.push((v, elems.len()));
        elems.push(Elem { kind: ElemKind::Node, rank: rank[v], node: v, edge: 0 });
    }
    let elem_of = |v: usize| prep.elem_of[v].unwrap_or(0);
    let mut chains: Vec<Option<Vec<usize>>> = vec![None; ne];
    for &(u, v, _, k, rev) in &dag {
        prep.info[k] = Some((u, v, rev));
        if g.edges[k].stroke == Stroke::Invisible {
            continue;
        }
        let (r1, r2) = (rank[u], rank[v]);
        let mut ch = vec![elem_of(u)];
        for r in r1 + 1..r2 {
            let kind =
                if has_text(&g.edges[k].label) && r == r1 + 1 && f == 2 { ElemKind::Label } else { ElemKind::Dummy };
            ch.push(elems.len());
            elems.push(Elem { kind, rank: r, node: 0, edge: k });
        }
        ch.push(elem_of(v));
        chains[k] = Some(ch);
    }

    // Lane candidates are back edges with a chain. Only their dummies can
    // leave the order, so if the rest is already over a cap, stop here.
    {
        let mut per_rank = vec![0usize; max_rank + 1];
        let mut maybe_lane = vec![false; elems.len()];
        for (k, ch) in chains.iter().enumerate() {
            if prep.back[k]
                && let Some(ch) = ch
            {
                for &e in &ch[1..ch.len() - 1] {
                    maybe_lane[e] = true;
                }
            }
        }
        for (e, el) in elems.iter().enumerate() {
            if !maybe_lane[e] {
                per_rank[el.rank] += 1;
            }
        }
        let total: usize = per_rank.iter().sum();
        if total > MAX_ELEMS || per_rank.iter().any(|&n| n > MAX_RANK_ELEMS) {
            return Err(NotDrawn::TooLarge);
        }
    }

    // E1. Starting order: breadth-first from the sources.
    let mut order: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    let mut placed = vec![false; elems.len()];
    let mut place = |e: usize, order: &mut Vec<Vec<usize>>| {
        if !placed[e] {
            placed[e] = true;
            order[elems[e].rank].push(e);
            true
        } else {
            false
        }
    };
    let mut from_node: Vec<Vec<usize>> = vec![Vec::new(); nn];
    for (k, ch) in chains.iter().enumerate() {
        if let Some(ch) = ch {
            from_node[elems[ch[0]].node].push(k);
        }
    }
    let mut seen = vec![false; nn];
    let mut q: VecDeque<usize> = nodes.iter().copied().filter(|&v| indeg[v] == 0).collect();
    for &v in &q {
        seen[v] = true;
    }
    while let Some(u) = q.pop_front() {
        place(elem_of(u), &mut order);
        for &k in &from_node[u] {
            let Some(ch) = &chains[k] else { continue };
            for &e in &ch[1..ch.len() - 1] {
                place(e, &mut order);
            }
            let last = ch[ch.len() - 1];
            place(last, &mut order);
            let t = elems[last].node;
            if !seen[t] {
                seen[t] = true;
                q.push_back(t);
            }
        }
    }
    for &v in &nodes {
        // A node reached only through invisible edges: the prototype placed
        // it but not its chains' dummies, and crashed. Place them after it.
        if place(elem_of(v), &mut order) {
            for &k in &from_node[v] {
                if let Some(ch) = &chains[k] {
                    for &e in &ch[1..ch.len() - 1] {
                        place(e, &mut order);
                    }
                }
            }
        }
    }

    let (mut up, mut down, _) = links(&chains, elems.len());

    // E2. Barycenter sweeps, keeping the order with the fewest crossings.
    let mut pos = vec![0usize; elems.len()];
    let positions = |order: &[Vec<usize>], pos: &mut [usize]| {
        for lst in order {
            for (i, &e) in lst.iter().enumerate() {
                pos[e] = i;
            }
        }
    };
    positions(&order, &mut pos);
    let mut best = order.clone();
    let mut best_c = total_crossings(&order, &down, &mut pos, work)?;
    let mut stale = 0;
    for it in 0..SWEEPS {
        let down_sweep = it % 2 == 0;
        positions(&order, &mut pos);
        let ranks: Vec<usize> = if down_sweep { (1..=max_rank).collect() } else { (0..max_rank).rev().collect() };
        for r in ranks {
            work.spend(order[r].len() as u64)?;
            let mut keyed: Vec<(f64, usize)> = order[r]
                .iter()
                .map(|&e| {
                    let nb = if down_sweep { &up[e] } else { &down[e] };
                    let key = if nb.is_empty() {
                        pos[e] as f64
                    } else {
                        nb.iter().map(|&x| pos[x]).sum::<usize>() as f64 / nb.len() as f64
                    };
                    (key, e)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            order[r] = keyed.into_iter().map(|(_, e)| e).collect();
            for (i, &e) in order[r].iter().enumerate() {
                pos[e] = i;
            }
        }
        let c = total_crossings(&order, &down, &mut pos, work)?;
        if c < best_c {
            best = order.clone();
            best_c = c;
            stale = 0;
        } else {
            stale += 1;
            if stale >= STALE_SWEEPS {
                break;
            }
        }
    }
    order = best;

    // E3. Transpose: swap neighbours while that removes crossings.
    for _ in 0..TRANSPOSE_PASSES {
        let mut changed = false;
        positions(&order, &mut pos);
        for lst in order.iter_mut() {
            for i in 0..lst.len().saturating_sub(1) {
                let (a, b) = (lst[i], lst[i + 1]);
                let cx = |a: usize, b: usize, work: &mut Work| -> Result<usize, NotDrawn> {
                    let mut c = 0;
                    for nb in [&up, &down] {
                        work.spend((nb[a].len() * nb[b].len()) as u64 + 1)?;
                        for &pa in &nb[a] {
                            for &pb in &nb[b] {
                                if pos[pa] > pos[pb] {
                                    c += 1;
                                }
                            }
                        }
                    }
                    Ok(c)
                };
                if cx(a, b, work)? > cx(b, a, work)? {
                    lst.swap(i, i + 1);
                    pos[a] = i + 1;
                    pos[b] = i;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    // F. Lanes for back edges, shortest span first.
    let mut loopers = vec![false; nn];
    for &k in &prep.selfloops {
        loopers[g.edges[k].from] = true;
    }
    let span = |k: usize| rank[g.edges[k].from].abs_diff(rank[g.edges[k].to]);
    // A lane draws one head, where it enters its target, so an edge with a
    // head at its source stays in the channels, which draw both.
    let mut cand: Vec<usize> =
        (0..ne).filter(|&k| prep.back[k] && chains[k].is_some() && g.edges[k].head_from == Head::None).collect();
    cand.sort_by_key(|&k| (span(k), k));
    let mut rejected = vec![false; ne];
    let mut accepted: Vec<usize>;
    loop {
        let mut lane_elem = vec![false; elems.len()];
        for &k in &cand {
            if !rejected[k]
                && let Some(ch) = &chains[k]
            {
                for &e in &ch[1..ch.len() - 1] {
                    lane_elem[e] = true;
                }
            }
        }
        let last = |e: usize| order[elems[e].rank].iter().rev().find(|&&x| !lane_elem[x]) == Some(&e);
        let mut sources = vec![false; nn];
        // By node: the stroke and head of the lanes it receives. Lanes into
        // one node share its last stretch of line and its head, so they must
        // ask for the same stroke and the same head.
        let mut targets: Vec<Option<(Stroke, Head)>> = vec![None; nn];
        accepted = Vec::new();
        let mut changed = false;
        for &k in &cand {
            if rejected[k] {
                continue;
            }
            work.spend(1)?;
            let (v, u) = (g.edges[k].from, g.edges[k].to);
            let end = (g.edges[k].stroke, g.edges[k].head_to);
            let ok = last(elem_of(v))
                && last(elem_of(u))
                && !loopers[v]
                && !loopers[u]
                && !sources[v]
                && targets[v].is_none()
                && !sources[u]
                && targets[u].is_none_or(|t| t == end);
            if ok {
                accepted.push(k);
                sources[v] = true;
                targets[u] = Some(end);
            } else {
                rejected[k] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for &k in &accepted {
        if let Some(ch) = chains[k].take() {
            for &e in &ch[1..ch.len() - 1] {
                let lst = &mut order[elems[e].rank];
                if let Some(i) = lst.iter().position(|&x| x == e) {
                    lst.remove(i);
                }
            }
        }
    }
    let link_edges;
    (up, down, link_edges) = links(&chains, elems.len());

    let widest = order.iter().map(Vec::len).max().unwrap_or(0);
    let total: usize = order.iter().map(Vec::len).sum();
    if widest > MAX_RANK_ELEMS || total > MAX_ELEMS {
        return Err(NotDrawn::TooLarge);
    }

    // Lines: the edges of each link split into the lines drawn for it, and
    // each element's lines to its neighbours.
    let link_lines: HashMap<(usize, usize), Vec<Line>> =
        link_edges.iter().map(|(&link, ks)| (link, split_lines(&prep, g, ks))).collect();
    let n_lines = |a: usize, b: usize| link_lines.get(&(a, b)).map_or(1, Vec::len);
    let mut down_lines: Vec<Vec<(usize, usize)>> = vec![Vec::new(); elems.len()];
    let mut up_lines: Vec<Vec<(usize, usize)>> = vec![Vec::new(); elems.len()];
    for (x, ys) in down.iter().enumerate() {
        work.spend(ys.len() as u64 * ys.len() as u64)?;
        for &y in ys {
            if !down_lines[x].iter().any(|&(z, _)| z == y) {
                down_lines[x].extend((0..n_lines(x, y)).map(|li| (y, li)));
            }
        }
    }
    for (y, xs) in up.iter().enumerate() {
        work.spend(xs.len() as u64 * xs.len() as u64)?;
        for &x in xs {
            if !up_lines[y].iter().any(|&(z, _)| z == x) {
                up_lines[y].extend((0..n_lines(x, y)).map(|li| (x, li)));
            }
        }
    }
    prep.max_rank = max_rank;
    prep.elems = elems;
    prep.order = order;
    prep.chains = chains;
    prep.up = up;
    prep.down = down;
    prep.link_lines = link_lines;
    prep.down_lines = down_lines;
    prep.up_lines = up_lines;
    prep.lanes = accepted;
    Ok(Prepared(Box::new(prep)))
}

type Links = (Vec<Vec<usize>>, Vec<Vec<usize>>, HashMap<(usize, usize), Vec<usize>>);

/// The up and down neighbours of each element, and the edges of each link.
fn links(chains: &[Option<Vec<usize>>], n: usize) -> Links {
    let mut up = vec![Vec::new(); n];
    let mut down = vec![Vec::new(); n];
    let mut link_edges: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (k, ch) in chains.iter().enumerate() {
        let Some(ch) = ch else { continue };
        for w in ch.windows(2) {
            down[w[0]].push(w[1]);
            up[w[1]].push(w[0]);
            link_edges.entry((w[0], w[1])).or_default().push(k);
        }
    }
    (up, down, link_edges)
}

/// Crossings between every pair of neighbouring ranks.
fn total_crossings(
    order: &[Vec<usize>],
    down: &[Vec<usize>],
    pos: &mut [usize],
    work: &mut Work,
) -> Result<usize, NotDrawn> {
    for lst in order {
        for (i, &e) in lst.iter().enumerate() {
            pos[e] = i;
        }
    }
    let mut total = 0;
    let mut tree: Vec<usize> = Vec::new();
    for r in 0..order.len().saturating_sub(1) {
        let mut pairs: Vec<(usize, usize)> = order[r]
            .iter()
            .flat_map(|&x| down[x].iter().map(move |&y| (x, y)))
            .map(|(x, y)| (pos[x], pos[y]))
            .collect();
        work.spend(pairs.len() as u64 + 1)?;
        pairs.sort_unstable();
        // Count pairs i < j with b[j] < b[i] (a Fenwick tree over b).
        let n = order[r + 1].len();
        tree.clear();
        tree.resize(n + 1, 0);
        for (seen, &(_, b)) in pairs.iter().enumerate() {
            let mut not_greater = 0;
            let mut i = b + 1;
            while i > 0 {
                not_greater += tree[i];
                i &= i - 1;
            }
            total += seen - not_greater;
            let mut i = b + 1;
            while i <= n {
                tree[i] += 1;
                i += i & i.wrapping_neg();
            }
        }
    }
    Ok(total)
}
