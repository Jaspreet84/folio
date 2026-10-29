//! Steps that run once per ladder step (plan section 6.2): sizes, ports,
//! placement, routing and painting, then the checks. Ported from
//! `flow_draw` in the prototype.
//!
//! Coordinates are `i64`: placement may push elements left of zero before
//! the drawing is shifted back.

use std::collections::{BTreeMap, HashMap, VecDeque};

use super::prep::{ElemKind, Line, Prepared, join_heads};
use super::{Dir, FlowGraph, Head, Node, Profile, Shape, Stroke};
use crate::diagram::canvas::{Bus, Canvas, E, N, Owner, S, W};
use crate::diagram::pre::{width, wrap_label};
use crate::diagram::{Charset, Checked, Drawing, Kind, LabelRef, NotDrawn, Problems, Role, Step, Work, check};

/// Draw self-loops in left-to-right steps, below the box (plan section
/// 6.2 M). Off, the prototype's behaviour: they are dropped and the step
/// fails its checks, so the diagram moves on to a top-down step. The
/// differential test against the prototype turns it off.
pub const LR_SELF_LOOPS: bool = true;

/// A router constraint that must hold.
const INF: i64 = 1_000_000_000;

/// Lines on one side of a node share a port when their key is equal:
/// stroke, head at that end, (port rule) the node whose fan-out they
/// belong to, and for a line with heads at both ends, its own (neighbour,
/// line), as it never shares a port.
type PortKey = (Stroke, Head, Option<usize>, Option<(usize, usize)>);

/// Draw `g` at one ladder step. The drawing may be wider than the page and
/// may have problems; the caller decides whether to show it.
pub fn draw(
    g: &FlowGraph,
    prep: &Prepared,
    step: Step,
    charset: Charset,
    work: &mut Work,
) -> Result<Checked, NotDrawn> {
    draw_with(g, prep, step, charset, LR_SELF_LOOPS, work)
}

/// [`draw`], choosing whether left-to-right self-loops are drawn (the
/// differential test compares against the prototype, which drops them).
pub fn draw_with(
    g: &FlowGraph,
    prep: &Prepared,
    step: Step,
    charset: Charset,
    lr_self_loops: bool,
    work: &mut Work,
) -> Result<Checked, NotDrawn> {
    // ASCII (M3) draws the same as Unicode for now.
    let _ = charset;
    let prof = Profile::new(step, g.dir.base());
    let painted = paint(g, prep, &prof, lr_self_loops, work)?;
    let cv = &painted.canvas;
    // The ends as drawn: an edge reversed to break a cycle runs from its
    // upper node to its lower one, like every other line a reader traces.
    let edges: Vec<Option<(usize, usize)>> = g
        .edges
        .iter()
        .enumerate()
        .map(|(k, e)| {
            (e.stroke != Stroke::Invisible)
                .then(|| prep.info.get(k).copied().flatten().map_or((e.from, e.to), |(u, v, _)| (u, v)))
        })
        .collect();
    // A line shared by several edges is drawn by its first edge; the check
    // counts the others there too.
    let mut also: Vec<Vec<usize>> = vec![Vec::new(); g.edges.len()];
    for l in prep.link_lines.values().flatten() {
        if let [k, rest @ ..] = l.edges.as_slice()
            && let Some(a) = also.get_mut(*k)
        {
            a.extend_from_slice(rest);
        }
    }
    let (_, bad_cells) = check::cell_check(cv, work)?;
    let false_pairs = check::flow_check(cv, &edges, &also, &painted.joins, work)?;
    let problems = Problems {
        text_over_line: check::text_over_line(cv),
        false_pairs,
        bad_cells,
        hard_conflicts: cv.hard_cycles,
        dropped: cv.dropped + cv.off_canvas,
    };
    // Rendering sorts every cell.
    work.spend(2 * cv.len() as u64)?;
    let rows = cv.render();
    let width = rows.iter().map(|r| width(&r.iter().map(|(_, s)| s.as_str()).collect::<String>())).max().unwrap_or(0);
    let drawing = Drawing {
        kind: Kind::Flowchart,
        title: None,
        step: Some(step),
        rows,
        width,
        labels: painted.labels,
        notes: Vec::new(),
    };
    Ok(Checked { drawing, problems })
}

/// A painted canvas and where its labels went.
pub struct Painted {
    pub canvas: Canvas,
    pub labels: Vec<LabelRef>,
    /// The line ends just above and below each edge label: a reader reads
    /// the line straight through the label, so the check joins them.
    pub joins: Vec<check::Join>,
}

/// A node's box at one step.
#[derive(Clone, Debug)]
struct NodeBox {
    lines: Vec<String>,
    w: i64,
    h: i64,
    shape: Shape,
    slant: Option<(char, char)>,
}

fn node_box(nd: &Node, prof: &Profile, td: bool, selfloop: bool) -> NodeBox {
    let lines = if nd.label.is_empty() { vec![String::new()] } else { wrap_label(&nd.label, prof.label_w) };
    let lw = lines.iter().map(|l| width(l)).max().unwrap_or(0) as i64;
    let bare = nd.shape == Shape::Bare;
    // A bare node is its text and nothing more, so its lines touch the
    // text. It never gets blank rows or columns to round its size.
    let mut inner = if bare { lw.max(1) } else { lw + 2 };
    if td && !bare && inner % 2 == 0 {
        inner += 1;
    }
    let w = inner + if bare { 0 } else { 2 } + if nd.shape == Shape::Subroutine { 2 } else { 0 };
    let n = lines.len() as i64;
    let cyl = i64::from(nd.shape == Shape::Cylinder);
    let mut h = n + if bare { 0 } else { 2 } + cyl;
    if td && selfloop && !bare {
        // The loop's rows are below a cylinder's rule.
        h = h.max(n + 3 + cyl);
    }
    if !td && !bare && h % 2 == 0 {
        h += 1;
    }
    NodeBox { lines, w, h, shape: nd.shape, slant: nd.slant }
}

/// The cells of a node's side that its ports may use, as the first and
/// last offset along the side: never a corner, a cylinder's rule or a
/// subroutine's bar. A bare node has no border, so every cell of its text.
fn port_cells(shape: Shape, td: bool, ext: i64) -> (i64, i64) {
    match shape {
        Shape::Bare => (0, ext - 1),
        Shape::Cylinder if !td => (2, ext - 2),
        Shape::Subroutine if td => (2, ext - 3),
        _ => (1, ext - 2),
    }
}

/// The middle of a node's inner rows (those below a cylinder's rule), where
/// top-down lanes leave and arrive.
fn mid_row(b: &NodeBox, y0: i64) -> i64 {
    let (top, bottom) = match b.shape {
        Shape::Bare => (y0, y0 + b.h - 1),
        Shape::Cylinder => (y0 + 2, y0 + b.h - 2),
        _ => (y0 + 1, y0 + b.h - 2),
    };
    top + (bottom - top).div_euclid(2)
}

/// One link of a chain between rank r and r+1.
#[derive(Clone, Debug)]
struct Piece {
    e1: usize,
    e2: usize,
    top: i64,
    bot: i64,
    k: usize,
    stroke: Stroke,
    /// Head drawn at the upper end, below the upper node.
    at: Option<Head>,
    /// Head drawn at the lower end, above the lower node.
    ab: Option<Head>,
    seg: usize,
    track: i64,
}

/// A group of pieces drawn as one bus.
struct Seg {
    pieces: Vec<usize>,
    tops: Vec<i64>,
    bots: Vec<i64>,
    l: i64,
    r: i64,
}

fn sorted_set(mut v: Vec<i64>) -> Vec<i64> {
    v.sort_unstable();
    v.dedup();
    v
}

fn intersects(a: &[i64], b: &[i64]) -> bool {
    a.iter().any(|x| b.binary_search(x).is_ok())
}

/// Python's `round` for a non-negative ratio: halves go to the even side.
fn round_ratio(num: i64, den: i64) -> i64 {
    (num as f64 / den as f64).round_ties_even() as i64
}

fn arrow(dir: Dir, down: bool) -> char {
    match (dir, down) {
        (Dir::LR, true) => '▸',
        (Dir::LR, false) => '◂',
        (_, true) => '▾',
        (_, false) => '▴',
    }
}

fn head_glyph(h: Head, arrow_glyph: char) -> char {
    match h {
        Head::Circle => 'o',
        Head::Cross => '✕',
        _ => arrow_glyph,
    }
}

/// Every cell of an orthogonal path through `pts`, in order.
fn walk(pts: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut seq = vec![pts[0]];
    for w in pts.windows(2) {
        let ((mut a, mut b), (a1, b1)) = (w[0], w[1]);
        while (a, b) != (a1, b1) {
            if a != a1 {
                a += (a1 - a).signum();
            } else {
                b += (b1 - b).signum();
            }
            if seq.last() != Some(&(a, b)) {
                seq.push((a, b));
            }
        }
    }
    seq
}

/// Paint an orthogonal path in screen coordinates (y, x).
fn seq_paint(cv: &mut Canvas, pts: &[(i64, i64)], stroke: Stroke, start_bit: u8, end_bit: u8) {
    let seq = walk(pts);
    let dir = |dy: i64, dx: i64| match (dy.signum(), dx.signum()) {
        (1, 0) => S,
        (-1, 0) => N,
        (0, 1) => E,
        (0, -1) => W,
        _ => 0,
    };
    for (i, &(y, x)) in seq.iter().enumerate() {
        let mut bits = 0;
        if i > 0 {
            bits |= dir(seq[i - 1].0 - y, seq[i - 1].1 - x);
        }
        if i + 1 < seq.len() {
            bits |= dir(seq[i + 1].0 - y, seq[i + 1].1 - x);
        }
        if i == 0 {
            bits |= start_bit;
        }
        if i == seq.len() - 1 {
            bits |= end_bit;
        }
        cv.line(y, x, bits, stroke);
    }
}

/// Paint `g` with profile `prof` (steps G to M), without the checks.
pub fn paint(
    g: &FlowGraph,
    prep: &Prepared,
    prof: &Profile,
    lr_self_loops: bool,
    work: &mut Work,
) -> Result<Painted, NotDrawn> {
    let ed = &g.edges;
    let elems = &prep.elems;
    let order = &prep.order;
    let rmax = prep.max_rank;
    // Neighbours by line: (element, line on that link). Most links have one
    // line; edges joining the same two nodes may share one or lie side by
    // side (see `Line`).
    let (up, down, chains) = (&prep.up_lines, &prep.down_lines, &prep.chains);
    let td = prof.dir != Dir::LR;
    let flow_bits = if td { N | S } else { W | E };
    let mut cv = Canvas::new(flow_bits);
    let mut labels_out: Vec<LabelRef> = Vec::new();
    let kind = |e: usize| elems[e].kind;
    let n_el = elems.len();
    let no_line = Line { edges: Vec::new(), stroke: Stroke::Solid, upper: Head::None, lower: Head::None };
    let line = |a: usize, b: usize, li: usize| prep.line(a, b, li).unwrap_or(&no_line);
    let line_ix = |a: usize, b: usize, k: usize| {
        prep.link_lines.get(&(a, b)).and_then(|ls| ls.iter().position(|l| l.edges.contains(&k))).unwrap_or(0)
    };

    // Self-loops by node, nodes in order of their first self-loop.
    let mut loop_nodes: Vec<(usize, Vec<usize>)> = Vec::new();
    for &k in &prep.selfloops {
        let a = ed[k].from;
        match loop_nodes.iter_mut().find(|(n, _)| *n == a) {
            Some((_, ks)) => ks.push(k),
            None => loop_nodes.push((a, vec![k])),
        }
    }
    let has_loop = |v: usize| loop_nodes.iter().any(|(n, _)| *n == v);

    // G. Sizes.
    let mut boxes: Vec<Option<NodeBox>> = g
        .nodes
        .iter()
        .enumerate()
        .map(|(k, nd)| (!nd.dead).then(|| node_box(nd, prof, td, td && has_loop(k))))
        .collect();
    let mut labels: Vec<Option<(Vec<String>, i64)>> = vec![None; elems.len()];
    for (e, el) in elems.iter().enumerate() {
        if el.kind == ElemKind::Label {
            let lines = wrap_label(ed[el.edge].label.as_deref().unwrap_or(""), prof.label_w);
            let lw = lines.iter().map(|l| width(l)).max().unwrap_or(0) as i64;
            labels[e] = Some((lines, lw));
        }
    }

    // H. Ports.
    let mut rpos = vec![0usize; elems.len()];
    for lst in order {
        for (i, &e) in lst.iter().enumerate() {
            rpos[e] = i;
        }
    }
    // By (node, neighbour below, line): how many lines share the node's
    // lower port with that line.
    let mut fanout: HashMap<(usize, usize, usize), usize> = HashMap::new();
    for (x0, el) in elems.iter().enumerate() {
        if el.kind != ElemKind::Node || down[x0].is_empty() {
            continue;
        }
        let key = |(z, li): (usize, usize)| {
            let l = line(x0, z, li);
            (l.stroke, l.upper, l.both_ways().then_some((z, li)))
        };
        for &(z, li) in &down[x0] {
            let n = down[x0].iter().filter(|&&z2| key(z2) == key((z, li))).count();
            fanout.insert((x0, z, li), n);
        }
    }
    // By (element, neighbour, line): the port's offset from the middle.
    // Every node's lower side comes first, so an upper side can follow the
    // order the ports have at the far end.
    let mut off: HashMap<(usize, usize, usize), i64> = HashMap::new();
    for far in [true, false] {
        for &(k_node, ne) in &prep.node_elem {
            let Some(b) = boxes[k_node].as_mut() else { continue };
            let nbmap = if far { down } else { up };
            let mut groups: Vec<(PortKey, Vec<(usize, usize)>)> = Vec::new();
            for &(y, li) in &nbmap[ne] {
                let l = if far { line(ne, y, li) } else { line(y, ne, li) };
                let mut key = (l.stroke, if far { l.upper } else { l.lower }, None, l.both_ways().then_some((y, li)));
                if !far && let Some(ch) = chains.get(l.owner()).and_then(Option::as_ref) {
                    // A link that belongs to another node's fan-out gets its
                    // own port (the port rule).
                    if let Some(mut j) = ch.iter().position(|&x| x == y) {
                        // A label sits on its line with no gap, so the bus
                        // runs on through it.
                        while j > 0 && matches!(kind(ch[j]), ElemKind::Dummy | ElemKind::Label) {
                            j -= 1;
                        }
                        if kind(ch[j]) == ElemKind::Node && j + 1 < ch.len() {
                            let lj = line_ix(ch[j], ch[j + 1], l.owner());
                            if fanout.get(&(ch[j], ch[j + 1], lj)).copied().unwrap_or(1) > 1 {
                                key.2 = Some(ch[j]);
                            }
                        }
                    }
                }
                match groups.iter_mut().find(|(k2, _)| *k2 == key) {
                    Some((_, ys)) => ys.push((y, li)),
                    None => groups.push((key, vec![(y, li)])),
                }
            }
            if groups.is_empty() {
                continue;
            }
            let mean =
                |ys: &Vec<(usize, usize)>| ys.iter().map(|&(y, _)| rpos[y]).sum::<usize>() as f64 / ys.len() as f64;
            groups.sort_by(|a, b| mean(&a.1).partial_cmp(&mean(&b.1)).unwrap_or(std::cmp::Ordering::Equal));
            if !far {
                // Groups that come from the same elements (lines of one link
                // with ports of their own) keep the order their ports have
                // at the upper end, so they don't cross.
                let from = |ys: &Vec<(usize, usize)>| {
                    let mut s: Vec<usize> = ys.iter().map(|&(y, _)| y).collect();
                    s.sort_unstable();
                    s.dedup();
                    s
                };
                let far_port = |ys: &Vec<(usize, usize)>| {
                    ys.iter().map(|&(y, li)| off.get(&(y, ne, li)).copied().unwrap_or(0)).sum::<i64>() as f64
                        / ys.len() as f64
                };
                let sets: Vec<Vec<usize>> = groups.iter().map(|(_, ys)| from(ys)).collect();
                let mut seen = vec![false; groups.len()];
                for i in 0..groups.len() {
                    if seen[i] {
                        continue;
                    }
                    let same: Vec<usize> = (i..groups.len()).filter(|&j| sets[j] == sets[i]).collect();
                    for &j in &same {
                        seen[j] = true;
                    }
                    if same.len() > 1 {
                        let mut moved: Vec<(PortKey, Vec<(usize, usize)>)> =
                            same.iter().map(|&j| groups[j].clone()).collect();
                        moved.sort_by(|a, b| {
                            far_port(&a.1).partial_cmp(&far_port(&b.1)).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        for (&j, gr) in same.iter().zip(moved) {
                            groups[j] = gr;
                        }
                    }
                }
            }
            let n = groups.len() as i64;
            let mut ext = if td { b.w } else { b.h };
            let (mut first, mut last) = port_cells(b.shape, td, ext);
            if n > 1 && last - first + 1 < 2 * n - 1 {
                if b.shape == Shape::Bare {
                    // Widening a bare node would put ports on blank cells
                    // beside its text.
                    cv.dropped += 1;
                } else {
                    ext += 2 * n - 1 - (last - first + 1);
                    if td {
                        b.w = ext;
                    } else {
                        b.h = ext;
                    }
                    (first, last) = port_cells(b.shape, td, ext);
                }
            }
            let c = (ext - 1).div_euclid(2);
            for (i, (_, ys)) in groups.iter().enumerate() {
                let p =
                    if n == 1 { c } else { first - 1 + round_ratio((i as i64 + 1) * (last - first + 2), n + 1) };
                let p = p.max(first).min(last);
                for &(y, li) in ys {
                    off.insert((ne, y, li), p - c);
                }
            }
        }
    }
    let boxes = boxes;
    let off_of = |a: usize, b: usize, li: usize| off.get(&(a, b, li)).copied().unwrap_or(0);

    // Extents across the flow (lo, hi) and along it (extr).
    let (mut lo, mut hi, mut extr) = (vec![0i64; n_el], vec![0i64; n_el], vec![0i64; n_el]);
    for (e, el) in elems.iter().enumerate() {
        match el.kind {
            ElemKind::Node => {
                let Some(b) = &boxes[el.node] else { continue };
                let ext = if td { b.w } else { b.h };
                lo[e] = (ext - 1).div_euclid(2);
                hi[e] = ext - 1 - lo[e];
                extr[e] = if td { b.h } else { b.w };
                if let Some((_, ks)) = loop_nodes.iter().find(|(n, _)| *n == el.node) {
                    if td {
                        let lw =
                            ks.iter().map(|&k| width(&loop_label(ed[k].label.as_deref()))).max().unwrap_or(0) as i64;
                        hi[e] += 3 + if lw > 0 { 1 + lw } else { 0 };
                    } else if lr_self_loops {
                        // Two rows below the box.
                        hi[e] += 2;
                    }
                }
            }
            ElemKind::Label => {
                let Some((lines, lw)) = &labels[e] else { continue };
                let ext = if td { *lw } else { lines.len() as i64 };
                lo[e] = (ext - 1).div_euclid(2);
                hi[e] = ext - 1 - lo[e];
                extr[e] = if td { lines.len() as i64 } else { lw + 2 };
            }
            ElemKind::Dummy => {}
        }
    }

    // I. Placement across the flow.
    let gap = |a: usize, b: usize| -> i64 {
        let (ka, kb) = (kind(a), kind(b));
        if ka == ElemKind::Node && kb == ElemKind::Node {
            if td { prof.node_gap as i64 } else { 1 }
        } else if ka == ElemKind::Label || kb == ElemKind::Label {
            if td { 2 } else { 1 }
        } else {
            1
        }
    };
    let sep = |a: usize, b: usize| hi[a] + gap(a, b) + lo[b] + 1;
    let mut pos = vec![0i64; n_el];
    let mut placed = vec![false; n_el];
    for lst in order {
        let mut x = 0;
        for (i, &e) in lst.iter().enumerate() {
            x = if i == 0 { lo[e] } else { x + sep(lst[i - 1], e) };
            pos[e] = x;
            placed[e] = true;
        }
    }
    let place_rank = |r: usize, ref_up: bool, pos: &mut Vec<i64>, work: &mut Work| -> Result<(), NotDrawn> {
        let lst = &order[r];
        let n = lst.len();
        let nbmap = if ref_up { up } else { down };
        let prio: Vec<usize> =
            lst.iter().map(|&e| if kind(e) != ElemKind::Node { 1_000_000 } else { nbmap[e].len() }).collect();
        let mut idx: Vec<usize> = (0..n).collect();
        idx.sort_by_key(|&i| (std::cmp::Reverse(prio[i]), i));
        let mut done = vec![false; n];
        for i in idx {
            let e = lst[i];
            let nbs = if kind(e) == ElemKind::Label { &down[e] } else { &nbmap[e] };
            if nbs.is_empty() {
                done[i] = true;
                continue;
            }
            work.spend(nbs.len() as u64)?;
            let mut vals: Vec<i64> =
                nbs.iter().map(|&(nb, li)| pos[nb] + off_of(nb, e, li) - off_of(e, nb, li)).collect();
            vals.sort_unstable();
            let m = vals.len();
            let d = if m % 2 == 1 { vals[m / 2] } else { (vals[m / 2 - 1] + vals[m / 2]).div_euclid(2) };
            let x = pos[e];
            if d > x {
                let (mut lim, mut acc) = (d, 0);
                for j in i + 1..n {
                    acc += sep(lst[j - 1], lst[j]);
                    if done[j] {
                        lim = lim.min(pos[lst[j]] - acc);
                        break;
                    }
                }
                pos[e] = x.max(lim);
                for j in i + 1..n {
                    work.spend(1)?;
                    let need = pos[lst[j - 1]] + sep(lst[j - 1], lst[j]);
                    if pos[lst[j]] < need {
                        pos[lst[j]] = need;
                    } else {
                        break;
                    }
                }
            } else if d < x {
                let (mut lim, mut acc) = (d, 0);
                for j in (0..i).rev() {
                    acc += sep(lst[j], lst[j + 1]);
                    if done[j] {
                        lim = lim.max(pos[lst[j]] + acc);
                        break;
                    }
                }
                pos[e] = x.min(lim);
                for j in (0..i).rev() {
                    work.spend(1)?;
                    let need = pos[lst[j + 1]] - sep(lst[j], lst[j + 1]);
                    if pos[lst[j]] > need {
                        pos[lst[j]] = need;
                    } else {
                        break;
                    }
                }
            }
            done[i] = true;
        }
        Ok(())
    };
    for _ in 0..3 {
        for r in 1..=rmax {
            place_rank(r, true, &mut pos, work)?;
        }
        for r in (0..rmax).rev() {
            place_rank(r, false, &mut pos, work)?;
        }
    }
    for r in 1..=rmax {
        place_rank(r, true, &mut pos, work)?;
    }
    let minx = (0..n_el).filter(|&e| placed[e]).map(|e| pos[e] - lo[e]).min().unwrap_or(0);
    for e in 0..n_el {
        if placed[e] {
            pos[e] -= minx;
        }
    }
    let pos = pos;
    let port = |e: usize, nb: usize, li: usize| pos[e] + off_of(e, nb, li);

    // J and K. Along the flow, and channel routing.
    let mut thick: Vec<i64> = order.iter().map(|lst| lst.iter().map(|&e| extr[e]).max().unwrap_or(0)).collect();
    if !td && lr_self_loops {
        // The loop's label sits on the row below the loop, starting 4
        // columns in (5 on a subroutine); the rank must be thick enough to
        // hold it and two blank cells after it, so it can't be read as the
        // label of a line in the next channel.
        for (k, ks) in &loop_nodes {
            if let Some(e) = prep.elem_of[*k] {
                let lw = ks.iter().map(|&k| width(&loop_label(ed[k].label.as_deref()))).max().unwrap_or(0) as i64;
                let sub = i64::from(g.nodes[*k].shape == Shape::Subroutine);
                let r = elems[e].rank;
                thick[r] = thick[r].max(extr[e]).max(if lw > 0 { 4 + sub + lw + 2 } else { 0 });
            }
        }
    }
    let thick = thick;
    // Lanes in the order they are painted: shortest span first.
    let rank_of = |v: usize| prep.elem_of[v].map(|e| elems[e].rank);
    let mut lanes = prep.lanes.clone();
    lanes.sort_by_key(|&k| rank_of(ed[k].from).unwrap_or(0).abs_diff(rank_of(ed[k].to).unwrap_or(0)));
    // A left-to-right lane runs below the drawing from the middle of v back
    // to the middle of u. Its label goes on that run, clear of the lanes
    // painted later that cross it, with a line cell at each end. By v's
    // rank: u's rank and middle, v's middle, the label's cells, and the
    // rank and middle of each node between them that a later lane leaves
    // or enters.
    type Room = (usize, i64, i64, i64, Vec<(usize, i64)>);
    let mut lane_room: Vec<Vec<Room>> = vec![Vec::new(); rmax + 1];
    let mid = |v: usize| boxes[v].as_ref().map_or(0, |b| (b.w - 1).div_euclid(2));
    if !td {
        for (i, &k) in lanes.iter().enumerate() {
            let lw = width(&loop_label(ed[k].label.as_deref())) as i64;
            let (v, u) = (ed[k].from, ed[k].to);
            let (Some(rv), Some(ru)) = (rank_of(v), rank_of(u)) else { continue };
            if lw == 0 || ru >= rv {
                continue;
            }
            let crossers = lanes[i + 1..]
                .iter()
                .flat_map(|&k2| [ed[k2].from, ed[k2].to])
                .filter_map(|w| rank_of(w).filter(|&rw| ru < rw && rw < rv).map(|rw| (rw, mid(w))))
                .collect();
            lane_room[rv].push((ru, mid(u), mid(v), lw + 2, crossers));
        }
    }
    let mut rs_ = vec![0i64; rmax + 1];
    let mut cs_ = vec![0i64; rmax + 1];
    // (rank, pieces, stem)
    let mut plans: Vec<(usize, Vec<Piece>, i64)> = Vec::new();
    let mut y = 0;
    for r in 0..=rmax {
        // Widen the channel before this rank when a lane's label needs it:
        // the label goes in the first gap between crossings that holds it,
        // or else after the last crossing.
        for (ru, mid_u, mid_v, n, crossers) in &lane_room[r] {
            let mut cols: Vec<i64> = crossers.iter().map(|&(rw, m)| rs_[rw] + m).collect();
            cols.sort_unstable();
            work.spend(cols.len() as u64 + 1)?;
            let mut start = rs_[*ru] + mid_u + 2;
            let mut fits = false;
            for c in cols {
                if c - start >= *n {
                    fits = true;
                    break;
                }
                start = start.max(c + 1);
            }
            if !fits {
                y = y.max(start + n + 1 - mid_v);
            }
        }
        rs_[r] = y;
        cs_[r] = y + thick[r];
        if r == rmax {
            break;
        }
        let mut pieces: Vec<Piece> = Vec::new();
        for &e in &order[r] {
            for &(t, li) in &down[e] {
                let l = line(e, t, li);
                pieces.push(Piece {
                    e1: e,
                    e2: t,
                    top: port(e, t, li),
                    bot: port(t, e, li),
                    k: l.owner(),
                    stroke: l.stroke,
                    at: (kind(e) == ElemKind::Node && l.upper != Head::None).then_some(l.upper),
                    ab: (kind(t) == ElemKind::Node && l.lower != Head::None).then_some(l.lower),
                    seg: 0,
                    track: 0,
                });
            }
        }
        // Fan-outs share a top port; the rest join by bottom port.
        let mut by_top: Vec<((usize, i64), Vec<usize>)> = Vec::new();
        let mut top_ix: HashMap<(usize, i64), usize> = HashMap::new();
        for (i, p) in pieces.iter().enumerate() {
            let key = (p.e1, p.top);
            match top_ix.get(&key) {
                Some(&j) => by_top[j].1.push(i),
                None => {
                    top_ix.insert(key, by_top.len());
                    by_top.push((key, vec![i]));
                }
            }
        }
        let mut seg_pieces: Vec<Vec<usize>> = Vec::new();
        let mut by_bot: Vec<Vec<usize>> = Vec::new();
        let mut bot_ix: HashMap<(usize, i64), usize> = HashMap::new();
        for (_, ps) in by_top {
            if ps.len() > 1 {
                seg_pieces.push(ps);
            } else {
                let p = &pieces[ps[0]];
                let key = (p.e2, p.bot);
                match bot_ix.get(&key) {
                    Some(&j) => by_bot[j].push(ps[0]),
                    None => {
                        bot_ix.insert(key, by_bot.len());
                        by_bot.push(ps);
                    }
                }
            }
        }
        seg_pieces.extend(by_bot);
        let mut segs: Vec<Seg> = Vec::with_capacity(seg_pieces.len());
        for (si, ps) in seg_pieces.into_iter().enumerate() {
            let tops = sorted_set(ps.iter().map(|&i| pieces[i].top).collect());
            let bots = sorted_set(ps.iter().map(|&i| pieces[i].bot).collect());
            let l = tops[0].min(bots[0]);
            let rr = tops[tops.len() - 1].max(bots[bots.len() - 1]);
            for &i in &ps {
                pieces[i].seg = si;
            }
            segs.push(Seg { pieces: ps, tops, bots, l, r: rr });
        }
        let ns: Vec<usize> = (0..segs.len()).filter(|&s| segs[s].l != segs[s].r).collect();
        let m = ns.len();
        let mut above: Vec<BTreeMap<usize, i64>> = vec![BTreeMap::new(); m];
        let cost = |a: &Seg, b: &Seg| {
            b.tops.iter().filter(|&&x| a.l < x && x < a.r).count() as i64
                + a.bots.iter().filter(|&&x| b.l < x && x < b.r).count() as i64
        };
        for i in 0..m {
            work.spend(m as u64)?;
            for j in i + 1..m {
                let (a, b) = (&segs[ns[i]], &segs[ns[j]]);
                if a.r + 1 < b.l || b.r + 1 < a.l {
                    continue;
                }
                work.spend((a.tops.len() + a.bots.len() + b.tops.len() + b.bots.len()) as u64)?;
                let hab = intersects(&a.tops, &b.bots);
                let hba = intersects(&b.tops, &a.bots);
                if hab && hba {
                    cv.hard_cycles += 1;
                }
                if hab && !hba {
                    above[i].insert(j, INF);
                } else if hba && !hab {
                    above[j].insert(i, INF);
                } else {
                    let (cab, cba) = (cost(a, b), cost(b, a));
                    if cab <= cba {
                        above[i].insert(j, cba - cab + if hab { INF } else { 0 });
                    } else {
                        above[j].insert(i, cab - cba);
                    }
                }
            }
        }
        // Break cycles: remove the lightest edge of each cycle found.
        cv.hard_cycles += break_cycles(&mut above, work)?;
        // Tracks by longest path.
        let mut track = vec![0i64; m];
        let mut indeg = vec![0usize; m];
        for a in &above {
            work.spend(a.len() as u64 + 1)?;
            for &b in a.keys() {
                indeg[b] += 1;
            }
        }
        let mut q: VecDeque<usize> = (0..m).filter(|&i| indeg[i] == 0).collect();
        while let Some(a) = q.pop_front() {
            for &b in above[a].keys() {
                track[b] = track[b].max(track[a] + 1);
                indeg[b] -= 1;
                if indeg[b] == 0 {
                    q.push_back(b);
                }
            }
        }
        let tracks = if m > 0 { track.iter().max().copied().unwrap_or(0) + 1 } else { 0 };
        let stem = i64::from(tracks > 0 || pieces.iter().any(|p| p.at.is_some()));
        let bottom = i64::from(pieces.iter().any(|p| kind(p.e2) == ElemKind::Node));
        let mut h = stem + tracks + bottom;
        let upper_nodes = order[r].iter().any(|&e| kind(e) != ElemKind::Dummy);
        let lower_nodes = order[r + 1].iter().any(|&e| kind(e) != ElemKind::Dummy);
        // A straight piece into a node shows its stroke in at least one
        // cell: a dotted or thick one, and one with a head at each end
        // (otherwise `◂▸` or `▴` over `▾`, with no line between).
        if tracks == 0
            && pieces.iter().any(|p| {
                (matches!(p.stroke, Stroke::Dotted | Stroke::Thick) || (p.at.is_some() && p.ab.is_some()))
                    && p.top == p.bot
                    && kind(p.e2) == ElemKind::Node
            })
        {
            h = h.max(2 + stem);
        }
        if td {
            if pieces.iter().any(|p| kind(p.e1) == ElemKind::Node && kind(p.e2) == ElemKind::Label) {
                h = h.max(1);
            }
        } else if !pieces.is_empty() {
            h = h.max(prof.lr_chan as i64);
        }
        if upper_nodes && lower_nodes {
            h = h.max(1);
        }
        for (i, &s) in ns.iter().enumerate() {
            for &p in &segs[s].pieces {
                pieces[p].track = track[i];
            }
        }
        plans.push((r, pieces, stem));
        y = cs_[r] + h;
    }

    // L. Painting the pieces.
    let scr = |rr: i64, oo: i64| if td { (rr, oo) } else { (oo, rr) };
    let apath = |cv: &mut Canvas, pts: &[(i64, i64)], stroke: Stroke, start_up: bool, end_down: bool| {
        let seq = walk(pts);
        let dbit = |dr: i64, d_o: i64| match (dr.signum(), d_o.signum(), td) {
            (1, 0, true) | (0, 1, false) => S,
            (-1, 0, true) | (0, -1, false) => N,
            (0, 1, true) | (1, 0, false) => E,
            (0, -1, true) | (-1, 0, false) => W,
            _ => 0,
        };
        for (i, &(rr, oo)) in seq.iter().enumerate() {
            let mut bits = 0;
            if i > 0 {
                bits |= dbit(seq[i - 1].0 - rr, seq[i - 1].1 - oo);
            }
            if i + 1 < seq.len() {
                bits |= dbit(seq[i + 1].0 - rr, seq[i + 1].1 - oo);
            }
            if i == 0 && start_up {
                bits |= dbit(-1, 0);
            }
            if i == seq.len() - 1 && end_down {
                bits |= dbit(1, 0);
            }
            let (yy, xx) = scr(rr, oo);
            cv.line(yy, xx, bits, stroke);
        }
    };
    let out_anchor = |e: usize| {
        let r = elems[e].rank;
        if kind(e) == ElemKind::Dummy && thick[r] == 0 {
            return cs_[r];
        }
        rs_[r] + if kind(e) != ElemKind::Dummy { extr[e] } else { thick[r] } - 1
    };
    let in_anchor = |e: usize| {
        let r = elems[e].rank;
        if kind(e) == ElemKind::Dummy && thick[r] == 0 { rs_[r] - 1 } else { rs_[r] }
    };
    // A bare node has no border to end on: a line without a head stops in
    // the cell next to the text, pointing at it.
    let bare = |v: usize| boxes[v].as_ref().is_some_and(|b| b.shape == Shape::Bare);
    let bare_el = |e: usize| kind(e) == ElemKind::Node && bare(elems[e].node);
    for (r, pieces, stem) in &plans {
        for p in pieces {
            cv.owner = Some(Owner { bus: Bus::Segment { rank: *r, index: p.seg }, edge: Some(p.k) });
            let mut rs = out_anchor(p.e1);
            let mut re = in_anchor(p.e2);
            let mut start_up = matches!(kind(p.e1), ElemKind::Dummy | ElemKind::Label);
            let mut end_down = matches!(kind(p.e2), ElemKind::Dummy | ElemKind::Label);
            if kind(p.e1) == ElemKind::Label {
                rs += 1;
            }
            if kind(p.e2) == ElemKind::Label {
                re -= 1;
            }
            if let Some(h) = p.at {
                rs += 1;
                let (yy, xx) = scr(rs, p.top);
                cv.glyph(yy, xx, head_glyph(h, arrow(prof.dir, false)), Role::Arrow);
            } else if bare_el(p.e1) {
                rs += 1;
                start_up = true;
            }
            if let Some(h) = p.ab {
                re -= 1;
                let (yy, xx) = scr(re, p.bot);
                cv.glyph(yy, xx, head_glyph(h, arrow(prof.dir, true)), Role::Arrow);
            } else if bare_el(p.e2) {
                re -= 1;
                end_down = true;
            }
            if p.top == p.bot {
                if rs > re {
                    continue;
                }
                apath(&mut cv, &[(rs, p.top), (re, p.top)], p.stroke, start_up, end_down);
            } else {
                let tr = cs_[*r] + stem + p.track;
                apath(&mut cv, &[(rs, p.top), (tr, p.top), (tr, p.bot), (re, p.bot)], p.stroke, start_up, end_down);
            }
        }
        work.spend(std::mem::take(&mut cv.steps))?;
    }
    // Dummies that pass through a rank with thickness.
    for (e, el) in elems.iter().enumerate() {
        if el.kind == ElemKind::Dummy && placed[e] && thick[el.rank] > 0 && !up[e].is_empty() {
            let r = el.rank;
            cv.owner = Some(Owner { bus: Bus::Dummy(e), edge: Some(el.edge) });
            apath(&mut cv, &[(rs_[r], pos[e]), (rs_[r] + thick[r] - 1, pos[e])], ed[el.edge].stroke, true, true);
        }
    }

    // Nodes.
    cv.owner = None;
    let mut rects: Vec<(i64, i64, i64, i64)> = vec![(0, 0, 0, 0); g.nodes.len()];
    for &(k, ne) in &prep.node_elem {
        let Some(b) = &boxes[k] else { continue };
        let r = elems[ne].rank;
        let (x0, y0) = if td { (pos[ne] - lo[ne], rs_[r]) } else { (rs_[r], pos[ne] - lo[ne]) };
        rects[k] = (x0, y0, b.w, b.h);
        let pieces = paint_box(&mut cv, x0, y0, b);
        labels_out.push(LabelRef { pieces });
    }

    // Edge labels.
    let mut joins = Vec::new();
    for (e, lab) in labels.iter().enumerate() {
        let Some((lines, lw)) = lab else { continue };
        if !placed[e] {
            continue;
        }
        let r = elems[e].rank;
        joins.push((scr(rs_[r] - 1, pos[e]), scr(rs_[r] + extr[e], pos[e])));
        let mut pieces = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            let lwi = width(l) as i64;
            if td {
                let (yy, xx) = (rs_[r] + i as i64, pos[e] - lo[e] + (lw - lwi).div_euclid(2));
                cv.put_text(yy, xx, l, Role::EdgeLabel, false);
                push_piece(&mut pieces, yy, xx, l);
            } else {
                let yy = pos[e] - lo[e] + i as i64;
                let pad = usize::try_from(lw - lwi + 1).unwrap_or(0);
                let txt = format!(" {l}{}", " ".repeat(pad));
                cv.put_text(yy, rs_[r], &txt, Role::EdgeLabel, false);
                push_piece(&mut pieces, yy, rs_[r] + 1, l);
            }
        }
        labels_out.push(LabelRef { pieces });
    }

    // M. Self-loops: one loop per node, in the stroke and with the head its
    // self-loops share.
    for (k, ks) in &loop_nodes {
        let (x0, y0, w, h) = rects[*k];
        let Some((stroke, head, lab)) = one_loop(g, ks) else {
            cv.dropped += 1;
            continue;
        };
        let shape = g.nodes[*k].shape;
        if shape == Shape::Bare {
            // The loop needs a border to leave and enter by.
            cv.dropped += 1;
            continue;
        }
        cv.owner = Some(Owner { bus: Bus::SelfLoop(ks[0]), edge: Some(ks[0]) });
        if td {
            let xr = x0 + w - 1;
            // Below a cylinder's rule, so the loop doesn't continue it.
            let ya = y0 + 1 + i64::from(shape == Shape::Cylinder);
            let yb = ya + 1;
            cv.line(ya, xr, E, stroke);
            cv.line(ya, xr + 1, W | E, stroke);
            cv.line(ya, xr + 2, W | S, stroke);
            cv.line(yb, xr + 2, N | W, stroke);
            if head == Head::None {
                cv.line(yb, xr + 1, W | E, stroke);
                cv.line(yb, xr, E, stroke);
            } else {
                cv.glyph(yb, xr + 1, head_glyph(head, '◂'), Role::Arrow);
            }
            if !lab.is_empty() {
                cv.put_text(ya, xr + 4, &lab, Role::EdgeLabel, false);
                labels_out.push(LabelRef { pieces: vec![(to_usize(ya), to_usize(xr + 4), lab)] });
            }
        } else if lr_self_loops {
            // The top-down pattern turned: out of the bottom border, round
            // and back up into it, with the label to the right.
            let yb = y0 + h - 1;
            // Right of a subroutine's bar, so the loop doesn't continue it.
            let xa = x0 + 1 + i64::from(shape == Shape::Subroutine);
            let xc = xa + 1;
            cv.line(yb, xa, S, stroke);
            cv.line(yb + 1, xa, N | S, stroke);
            cv.line(yb + 2, xa, N | E, stroke);
            cv.line(yb + 2, xc, W | N, stroke);
            if head == Head::None {
                cv.line(yb + 1, xc, N | S, stroke);
                cv.line(yb, xc, S, stroke);
            } else {
                cv.glyph(yb + 1, xc, head_glyph(head, '▴'), Role::Arrow);
            }
            if !lab.is_empty() {
                cv.put_text(yb + 2, xc + 2, &lab, Role::EdgeLabel, false);
                labels_out.push(LabelRef { pieces: vec![(to_usize(yb + 2), to_usize(xc + 2), lab)] });
            }
        } else {
            cv.dropped += 1;
        }
    }

    // M. Lanes for back edges, shortest span first.
    // Left-to-right lane labels, placed once every lane is drawn: (owner,
    // row, first and last column the label may use, label, its LabelRef).
    let mut lr_labels: Vec<(Owner, i64, i64, i64, String, usize)> = Vec::new();
    for k in lanes {
        let (v, u) = (ed[k].from, ed[k].to);
        let (Some(bv), Some(bu)) = (&boxes[v], &boxes[u]) else { continue };
        let (vx, vy, vw, vh) = rects[v];
        let (ux, uy, uw, uh) = rects[u];
        let lab = loop_label(ed[k].label.as_deref());
        let lw = width(&lab) as i64;
        let stroke = ed[k].stroke;
        let head = ed[k].head_to;
        let has_head = head != Head::None;
        let owner = Owner { bus: Bus::Lane(u), edge: Some(k) };
        cv.owner = Some(owner);
        work.spend(2 * cv.positions().len() as u64)?;
        // A lane leaves through v's border (or from beside a bare node's
        // text). It ends with a head next to u, or without one on u's
        // border, which becomes a tee (beside a bare node's text).
        let (bare_v, bare_u) = (i64::from(bare(v)), i64::from(bare(u)));
        let (end, end_bit) = match (has_head, bare_u == 1) {
            (true, _) => (2, if td { W } else { N }),
            (false, true) => (1, if td { W } else { N }),
            (false, false) => (0, 0),
        };
        if td {
            let (ym_v, ym_u) = (mid_row(bv, vy), mid_row(bu, uy));
            let (xr_v, xr_u) = (vx + vw - 1, ux + uw - 1);
            let far = cv.positions().iter().filter(|&&(yy, _)| ym_u <= yy && yy <= ym_v).map(|&(_, x)| x).max();
            // The label goes on the exit stub, past every line the stub
            // crosses, so it covers none of them.
            let crossed = cv.positions().iter().filter(|&&(yy, xx)| yy == ym_v && xx > xr_v).map(|&(_, x)| x).max();
            let lab_x = crossed.map_or(xr_v + 2, |x| x + 2).max(xr_v + 2);
            let lane = (far.unwrap_or(0) + 2).max(if lw > 0 { lab_x + lw + 3 } else { xr_v + 3 });
            let pts = [(ym_v, xr_v + bare_v), (ym_v, lane), (ym_u, lane), (ym_u, xr_u + end)];
            seq_paint(&mut cv, &pts, stroke, 0, end_bit);
            if has_head {
                cv.glyph(ym_u, xr_u + 1, head_glyph(head, '◂'), Role::Arrow);
            }
            if lw > 0 {
                cv.put_text(ym_v, lab_x, &format!(" {lab} "), Role::EdgeLabel, true);
                labels_out.push(LabelRef { pieces: vec![(to_usize(ym_v), to_usize(lab_x + 1), lab)] });
            }
        } else {
            let (xm_v, xm_u) = (vx + (vw - 1).div_euclid(2), ux + (uw - 1).div_euclid(2));
            let (yb_v, yb_u) = (vy + vh - 1, uy + uh - 1);
            let far = cv.positions().iter().filter(|&&(_, xx)| xm_u <= xx && xx <= xm_v).map(|&(yy, _)| yy).max();
            let lane = far.unwrap_or(0) + 2;
            let pts = [(yb_v + bare_v, xm_v), (lane, xm_v), (lane, xm_u), (yb_u + end, xm_u)];
            seq_paint(&mut cv, &pts, stroke, 0, end_bit);
            if has_head {
                cv.glyph(yb_u + 1, xm_u, head_glyph(head, '▴'), Role::Arrow);
            }
            if lw > 0 {
                // Between the corners, with a line cell on each side.
                lr_labels.push((owner, lane, xm_u + 2, xm_v - 2, lab, labels_out.len()));
                labels_out.push(LabelRef::default());
            }
        }
    }
    // A left-to-right label goes at the first place on its lane's run where
    // it covers no other line (a later lane may cross the run). With no
    // such place, the step fails.
    for (owner, y, first, last, lab, li) in lr_labels {
        let text = format!(" {lab} ");
        let n = width(&text) as i64;
        work.spend(((last - first + 1).max(0) * n) as u64)?;
        let at = (first..=last - n + 1).find(|&x| (x..x + n).all(|xx| cv.only_line_of(y, xx, owner)));
        let Some(x) = at else {
            cv.dropped += 1;
            continue;
        };
        cv.owner = Some(owner);
        cv.put_text(y, x, &text, Role::EdgeLabel, true);
        labels_out[li].pieces.push((to_usize(y), to_usize(x + 1), lab));
    }
    cv.owner = None;
    work.spend(std::mem::take(&mut cv.steps))?;
    Ok(Painted { canvas: cv, labels: labels_out, joins })
}

/// A self-loop or lane label on one line.
fn loop_label(label: Option<&str>) -> String {
    label.unwrap_or("").replace('\n', " ")
}

/// The stroke, head and label of the one loop drawn for a node's
/// self-loops `ks`. `None` when one loop can't show them all: their strokes
/// differ, as do their heads or labels, or one has a head where it leaves.
fn one_loop(g: &FlowGraph, ks: &[usize]) -> Option<(Stroke, Head, String)> {
    let stroke = g.edges[*ks.first()?].stroke;
    let (mut head, mut label) = (Head::None, String::new());
    for &k in ks {
        let e = &g.edges[k];
        if e.stroke != stroke || e.head_from != Head::None {
            return None;
        }
        head = join_heads(head, e.head_to)?;
        let l = loop_label(e.label.as_deref());
        if label.is_empty() {
            label = l;
        } else if !l.is_empty() && l != label {
            return None;
        }
    }
    Some((stroke, head, label))
}

fn to_usize(v: i64) -> usize {
    usize::try_from(v).unwrap_or(0)
}

fn push_piece(pieces: &mut Vec<(usize, usize, String)>, y: i64, x: i64, text: &str) {
    if !text.is_empty() {
        pieces.push((to_usize(y), to_usize(x), text.to_string()));
    }
}

/// Break the cycles of the "above" graph. A depth-first search from each
/// segment in order finds a cycle; its lightest edge (the first of them,
/// in path order) is removed, and the search goes on. This is the
/// prototype's rule, which searched again from the start after each
/// removal. Going on gives the same result, because a fresh search would
/// retrace the same steps up to the removed edge: after a back edge it is
/// where it stands; after a tree edge a → b, it is back at a, with b and
/// everything found after b not yet found. Returns the hard edges removed.
fn break_cycles(above: &mut [BTreeMap<usize, i64>], work: &mut Work) -> Result<usize, NotDrawn> {
    let m = above.len();
    let mut color = vec![0u8; m];
    let mut parent = vec![usize::MAX; m];
    // Segments in the order found, and each one's place in that order.
    let mut found: Vec<usize> = Vec::new();
    let mut found_at = vec![usize::MAX; m];
    let mut hard = 0;
    for s0 in 0..m {
        if color[s0] != 0 {
            continue;
        }
        color[s0] = 1;
        found_at[s0] = found.len();
        found.push(s0);
        // (segment, the lowest neighbour it hasn't looked at)
        let mut stack: Vec<(usize, usize)> = vec![(s0, 0)];
        while let Some(top) = stack.last_mut() {
            work.spend(1)?;
            let u = top.0;
            let Some(v) = above[u].range(top.1..).next().map(|(&v, _)| v) else {
                color[u] = 2;
                stack.pop();
                continue;
            };
            top.1 = v + 1;
            if color[v] == 0 {
                color[v] = 1;
                parent[v] = u;
                found_at[v] = found.len();
                found.push(v);
                stack.push((v, 0));
                continue;
            }
            if color[v] != 1 {
                continue;
            }
            // A cycle: v, down the tree to u, and back to v.
            let mut path = vec![v];
            let mut x = u;
            while x != v {
                work.spend(1)?;
                path.push(x);
                x = parent[x];
                if x == usize::MAX {
                    break;
                }
            }
            path.reverse();
            let mut best: Option<(usize, usize, i64)> = None;
            for i in 0..path.len() {
                let (a, b) = (path[i], path[(i + 1) % path.len()]);
                let w = above[a].get(&b).copied().unwrap_or(0);
                if best.is_none_or(|(_, _, bw)| w < bw) {
                    best = Some((a, b, w));
                }
            }
            let Some((a, b, w)) = best else { continue };
            if w >= INF {
                hard += 1;
            }
            above[a].remove(&b);
            if (a, b) != (u, v) && found_at[b] != usize::MAX {
                let i = found_at[b];
                work.spend((found.len() - i) as u64)?;
                for &x in &found[i..] {
                    color[x] = 0;
                    found_at[x] = usize::MAX;
                }
                found.truncate(i);
                while stack.last().is_some_and(|&(x, _)| x != a) {
                    stack.pop();
                }
            }
        }
    }
    Ok(hard)
}

/// Paint a node's box and label; returns where the label's lines went.
fn paint_box(cv: &mut Canvas, x0: i64, y0: i64, b: &NodeBox) -> Vec<(usize, usize, String)> {
    let (w, h, shape) = (b.w, b.h, b.shape);
    let (x1, y1) = (x0 + w - 1, y0 + h - 1);
    let bare = shape == Shape::Bare;
    let solid = Stroke::Solid;
    if !bare {
        for x in x0 + 1..x1 {
            cv.line(y0, x, W | E, solid);
            cv.line(y1, x, W | E, solid);
        }
        for y in y0 + 1..y1 {
            cv.line(y, x0, N | S, solid);
            cv.line(y, x1, N | S, solid);
        }
        match shape {
            Shape::Round | Shape::Stadium | Shape::Circle | Shape::DCircle | Shape::Cylinder => {
                cv.glyph(y0, x0, '╭', Role::Border);
                cv.glyph(y0, x1, '╮', Role::Border);
                cv.glyph(y1, x0, '╰', Role::Border);
                cv.glyph(y1, x1, '╯', Role::Border);
            }
            Shape::Decision | Shape::Hexagon => {
                cv.glyph(y0, x0, '╱', Role::Border);
                cv.glyph(y0, x1, '╲', Role::Border);
                cv.glyph(y1, x0, '╲', Role::Border);
                cv.glyph(y1, x1, '╱', Role::Border);
            }
            _ => {
                cv.line(y0, x0, S | E, solid);
                cv.line(y0, x1, S | W, solid);
                cv.line(y1, x0, N | E, solid);
                cv.line(y1, x1, N | W, solid);
            }
        }
        if shape == Shape::Subroutine {
            for y in y0..=y1 {
                let bits = if y == y0 {
                    S
                } else if y == y1 {
                    N
                } else {
                    N | S
                };
                cv.line(y, x0 + 1, bits, solid);
                cv.line(y, x1 - 1, bits, solid);
            }
        }
        if shape == Shape::Cylinder {
            cv.line(y0 + 1, x0, E, solid);
            cv.line(y0 + 1, x1, W, solid);
            for x in x0 + 1..x1 {
                cv.line(y0 + 1, x, W | E, solid);
            }
        }
    }
    let sub = shape == Shape::Subroutine;
    let top = y0 + if bare { 0 } else { 1 } + if shape == Shape::Cylinder { 1 } else { 0 };
    let inner_l = x0 + if bare { 0 } else { 1 } + if sub { 1 } else { 0 };
    let inner_w = w - if bare { 0 } else { 2 } - if sub { 2 } else { 0 };
    let end = if bare { y1 + 1 } else { y1 };
    for y in top..end {
        for x in inner_l..inner_l + inner_w {
            cv.put_text(y, x, " ", Role::NodeText, false);
        }
    }
    let n = b.lines.len() as i64;
    let mut pieces = Vec::new();
    for (i, l) in b.lines.iter().enumerate() {
        let (yy, xx) = (top + i as i64, inner_l + (inner_w - width(l) as i64).div_euclid(2));
        cv.put_text(yy, xx, l, Role::NodeText, false);
        push_piece(&mut pieces, yy, xx, l);
    }
    let mid = top + (n - 1).div_euclid(2);
    for y in top..top + n {
        match shape {
            Shape::Stadium | Shape::Circle | Shape::DCircle => {
                cv.glyph(y, x0, '(', Role::Border);
                cv.glyph(y, x1, ')', Role::Border);
            }
            Shape::Decision | Shape::Hexagon if y == mid => {
                cv.glyph(y, x0, '<', Role::Border);
                cv.glyph(y, x1, '>', Role::Border);
            }
            Shape::Slant | Shape::Asym => {
                if let Some((l, r)) = b.slant {
                    cv.glyph(y, x0, l, Role::Border);
                    if r != '│' {
                        cv.glyph(y, x1, r, Role::Border);
                    }
                }
            }
            _ => {}
        }
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::flow::{Edge, prep};
    use crate::diagram::{WORK_LIMIT, ladder};

    fn node(id: &str, label: &str, shape: Shape) -> Node {
        let slant = match shape {
            Shape::Slant => Some(('/', '/')),
            Shape::Asym => Some(('>', '│')),
            _ => None,
        };
        Node { id: id.into(), label: label.into(), shape, cluster: None, slant, explicit: true, dead: false }
    }

    fn rect(id: &str) -> Node {
        node(id, id, Shape::Rect)
    }

    fn edge(from: usize, to: usize, label: Option<&str>) -> Edge {
        Edge {
            from,
            to,
            stroke: Stroke::Solid,
            head_from: Head::None,
            head_to: Head::Arrow,
            label: label.map(str::to_string),
            min_len: 1,
            from_cluster: None,
            to_cluster: None,
        }
    }

    fn graph(dir: Dir, nodes: Vec<Node>, edges: Vec<Edge>) -> FlowGraph {
        let ids = nodes.iter().enumerate().map(|(i, n)| (n.id.clone(), i)).collect();
        FlowGraph { dir, nodes, edges, clusters: Vec::new(), simplified: Default::default(), ids }
    }

    /// Every ladder step: (step, rows, problems).
    fn steps(g: &FlowGraph) -> Vec<(Step, Vec<String>, Problems)> {
        steps_with(g, LR_SELF_LOOPS)
    }

    /// Every ladder step, choosing whether left-to-right self-loops are drawn.
    fn steps_with(g: &FlowGraph, lr_self_loops: bool) -> Vec<(Step, Vec<String>, Problems)> {
        let mut work = Work::new(WORK_LIMIT);
        let p = prep::prepare(g, &mut work).expect("prepared");
        ladder(g.dir)
            .iter()
            .map(|&step| {
                let c = draw_with(g, &p, step, Charset::Unicode, lr_self_loops, &mut work).expect("drawn");
                let rows = c.drawing.text().lines().map(str::to_string).collect();
                (step, rows, c.problems)
            })
            .collect()
    }

    fn rows_at(g: &FlowGraph, step: Step) -> Vec<String> {
        steps(g).into_iter().find(|s| s.0 == step).expect("step").1
    }

    fn assert_clean(g: &FlowGraph) {
        for (step, rows, problems) in steps(g) {
            assert!(!problems.any(), "{step:?}: {problems:?}\n{}", rows.join("\n"));
        }
    }

    #[test]
    fn single_node() {
        let g = graph(Dir::TD, vec![node("A", "Start", Shape::Rect)], vec![]);
        for (_, rows, problems) in steps(&g) {
            assert_eq!(rows, ["┌───────┐", "│ Start │", "└───────┘"]);
            assert!(!problems.any());
        }
    }

    #[test]
    fn chain_top_down() {
        let g = graph(Dir::TD, vec![rect("A"), rect("B"), rect("C")], vec![edge(0, 1, None), edge(1, 2, None)]);
        assert_clean(&g);
        let want = ["┌───┐", "│ A │", "└─┬─┘", "  ▾", "┌───┐", "│ B │", "└─┬─┘", "  ▾", "┌───┐", "│ C │", "└───┘"];
        assert_eq!(rows_at(&g, Step::Normal), want);
    }

    #[test]
    fn chain_left_to_right() {
        let g = graph(Dir::LR, vec![rect("A"), rect("B"), rect("C")], vec![edge(0, 1, None), edge(1, 2, None)]);
        assert_clean(&g);
        assert_eq!(
            rows_at(&g, Step::Normal),
            ["┌───┐    ┌───┐    ┌───┐", "│ A ├───▸│ B ├───▸│ C │", "└───┘    └───┘    └───┘"]
        );
        assert_eq!(rows_at(&g, Step::Compact), ["┌───┐  ┌───┐  ┌───┐", "│ A ├─▸│ B ├─▸│ C │", "└───┘  └───┘  └───┘"]);
        assert_eq!(rows_at(&g, Step::Turned)[3], "  ▾");
    }

    #[test]
    fn decision_with_two_labeled_branches() {
        let g = graph(
            Dir::TD,
            vec![node("A", "Ok?", Shape::Decision), node("B", "Ship", Shape::Rect), node("C", "Fix", Shape::Rect)],
            vec![edge(0, 1, Some("yes")), edge(0, 2, Some("no"))],
        );
        assert_clean(&g);
        let want = [
            "      ╱─────╲",
            "      < Ok? >",
            "      ╲──┬──╱",
            "         │",
            "    ┌────┴─────┐",
            "   yes         no",
            "    ▾          ▾",
            "┌───────┐   ┌─────┐",
            "│ Ship  │   │ Fix │",
            "└───────┘   └─────┘",
        ];
        assert_eq!(rows_at(&g, Step::Normal), want);
        let mut work = Work::new(WORK_LIMIT);
        let p = prep::prepare(&g, &mut work).unwrap();
        let d = draw(&g, &p, Step::Normal, Charset::Unicode, &mut work).unwrap().drawing;
        assert!(d.rows[5].iter().any(|(r, s)| *r == Role::EdgeLabel && s.contains("yes")));
        assert_eq!(d.rows[6], [(Role::Arrow, "    ▾          ▾".to_string())]);
        assert!(d.rows[1].iter().any(|(r, s)| *r == Role::NodeText && s.contains("Ok?")));
        assert!(d.rows[4].iter().any(|(r, s)| *r == Role::Edge && s.contains('┴')));
        assert!(d.rows[0].iter().all(|(r, _)| *r == Role::Border));
        assert_eq!(d.width, 19);
        // Three node labels and two edge labels.
        assert_eq!(d.labels.len(), 5);
        assert_eq!(d.labels[3].pieces, [(5, 3, "yes".to_string())]);
    }

    #[test]
    fn cycle_gets_a_lane() {
        let g = graph(
            Dir::TD,
            vec![rect("A"), rect("B"), rect("C")],
            vec![edge(0, 1, None), edge(1, 2, None), edge(2, 0, None)],
        );
        assert_clean(&g);
        let want = [
            "┌───┐",
            "│ A │◂─┐",
            "└─┬─┘  │",
            "  ▾    │",
            "┌───┐  │",
            "│ B │  │",
            "└─┬─┘  │",
            "  ▾    │",
            "┌───┐  │",
            "│ C ├──┘",
            "└───┘",
        ];
        assert_eq!(rows_at(&g, Step::Normal), want);
    }

    #[test]
    fn self_loop_top_down() {
        let g = graph(
            Dir::TD,
            vec![node("A", "Retry", Shape::Rect), rect("B")],
            vec![edge(0, 0, Some("again")), edge(0, 1, None)],
        );
        assert_clean(&g);
        let want =
            ["┌───────┐", "│ Retry ├─┐ again", "│       │◂┘", "└───┬───┘", "    ▾", "  ┌───┐", "  │ B │", "  └───┘"];
        assert_eq!(rows_at(&g, Step::Normal), want);
    }

    #[test]
    fn self_loop_left_to_right_dropped_as_in_the_prototype() {
        let g = graph(
            Dir::LR,
            vec![node("A", "Retry", Shape::Rect), rect("B")],
            vec![edge(0, 0, Some("again")), edge(0, 1, None)],
        );
        for (step, rows, problems) in steps_with(&g, false) {
            if step.turned() {
                assert!(!problems.any(), "{step:?}");
                assert_eq!(rows[1], "│ Retry ├─┐ again");
            } else {
                assert_eq!(problems, Problems { dropped: 1, ..Problems::default() }, "{step:?}");
                if step == Step::Normal {
                    assert_eq!(rows, ["┌───────┐    ┌───┐", "│ Retry ├───▸│ B │", "└───────┘    └───┘"]);
                }
            }
        }
    }

    #[test]
    fn self_loop_left_to_right_below_the_box() {
        let g = graph(
            Dir::LR,
            vec![node("A", "Retry", Shape::Rect), rect("B")],
            vec![edge(0, 0, Some("again")), edge(0, 1, None)],
        );
        assert_clean(&g);
        let want = ["┌───────┐      ┌───┐", "│ Retry ├─────▸│ B │", "└┬──────┘      └───┘", " │▴", " └┘ again"];
        assert_eq!(rows_at(&g, Step::Normal), want);
        // A long label widens the rank so the next channel starts after it.
        let g = graph(
            Dir::LR,
            vec![node("A", "Retry", Shape::Rect), rect("B")],
            vec![edge(0, 0, Some("try it once more")), edge(0, 1, None)],
        );
        assert_clean(&g);
        let want = [
            "┌───────┐                 ┌───┐",
            "│ Retry ├────────────────▸│ B │",
            "└┬──────┘                 └───┘",
            " │▴",
            " └┘ try it once more",
        ];
        assert_eq!(rows_at(&g, Step::Normal), want);
    }

    #[test]
    fn parallel_and_opposite_edges() {
        let g = graph(Dir::TD, vec![rect("A"), rect("B")], vec![edge(0, 1, None), edge(0, 1, None), edge(1, 0, None)]);
        assert_clean(&g);
        assert_eq!(
            rows_at(&g, Step::Normal),
            ["┌───┐", "│ A │◂─┐", "└─┬─┘  │", "  ▾    │", "┌───┐  │", "│ B ├──┘", "└───┘"]
        );
    }

    /// An edge with a stroke and heads.
    fn styled(from: usize, to: usize, stroke: Stroke, head_from: Head, head_to: Head) -> Edge {
        Edge { stroke, head_from, head_to, ..edge(from, to, None) }
    }

    #[test]
    fn two_cycle_gets_a_lane() {
        let g = graph(
            Dir::TD,
            vec![node("idle", "idle", Shape::Rect), node("running", "running", Shape::Rect)],
            vec![edge(0, 1, None), edge(1, 0, None)],
        );
        assert_clean(&g);
        let want = [
            " ┌───────┐",
            " │ idle  │◂──┐",
            " └───┬───┘   │",
            "     ▾       │",
            "┌─────────┐  │",
            "│ running ├──┘",
            "└─────────┘",
        ];
        assert_eq!(rows_at(&g, Step::Normal), want);
    }

    #[test]
    fn opposite_edges_share_a_line_top_down() {
        // B's self-loop keeps B → A out of a lane, so both edges are on the
        // link between A and B: one line with a head at each end.
        let g = graph(Dir::TD, vec![rect("A"), rect("B")], vec![edge(0, 1, None), edge(1, 0, None), edge(1, 1, None)]);
        assert_clean(&g);
        let want = ["┌───┐", "│ A │", "└───┘", "  ▴", "  │", "  ▾", "┌───┐", "│ B ├─┐", "│   │◂┘", "└───┘"];
        assert_eq!(rows_at(&g, Step::Normal), want);
        // A head at both ends of one edge looks the same, and it isn't a lane
        // (a lane has no head where it leaves).
        let both = styled(1, 0, Stroke::Solid, Head::Arrow, Head::Arrow);
        let g = graph(Dir::TD, vec![rect("A"), rect("B")], vec![edge(0, 1, None), both]);
        assert_clean(&g);
        assert_eq!(rows_at(&g, Step::Normal), ["┌───┐", "│ A │", "└───┘", "  ▴", "  │", "  ▾", "┌───┐", "│ B │", "└───┘"]);
    }

    #[test]
    fn opposite_edges_share_a_line_left_to_right() {
        // B isn't last in its rank, so B → A gets no lane.
        let g = graph(
            Dir::LR,
            vec![rect("A"), rect("B"), rect("C")],
            vec![edge(0, 1, None), edge(1, 0, None), edge(0, 2, None)],
        );
        assert_clean(&g);
        let want = [
            "┌───┐    ┌───┐",
            "│ A │◂──▸│ B │",
            "│   │    └───┘",
            "│   ├─┐",
            "└───┘ │  ┌───┐",
            "      └─▸│ C │",
            "         └───┘",
        ];
        assert_eq!(rows_at(&g, Step::Normal), want);
        assert_eq!(rows_at(&g, Step::Compact)[1], "│ A │◂─▸│ B │");
    }

    #[test]
    fn opposite_and_mixed_edges_left_to_right() {
        // A → B and B → A share a line; A -.-> C and A --> C lie side by side.
        let g = graph(
            Dir::LR,
            vec![rect("A"), rect("B"), rect("C")],
            vec![
                edge(0, 1, None),
                edge(1, 0, None),
                styled(0, 2, Stroke::Dotted, Head::None, Head::Arrow),
                edge(0, 2, None),
            ],
        );
        assert_clean(&g);
        let want = [
            "         ┌───┐",
            "      ┌─▸│ B │",
            "      │  └───┘",
            "┌───┐ │",
            "│ A │ │  ┌───┐",
            "│   │◂┘┌▸│ C │",
            "│   ├┄┄┘ │   │",
            "│   ├───▸│   │",
            "│   │    └───┘",
            "└───┘",
        ];
        assert_eq!(rows_at(&g, Step::Normal), want);
    }

    #[test]
    fn duplicate_edges_draw_as_one() {
        let one = graph(Dir::TD, vec![rect("A"), rect("B")], vec![edge(0, 1, None)]);
        let two = graph(Dir::TD, vec![rect("A"), rect("B")], vec![edge(0, 1, None), edge(0, 1, None)]);
        assert_clean(&two);
        let rows = |g: &FlowGraph| steps(g).into_iter().map(|s| s.1).collect::<Vec<_>>();
        assert_eq!(rows(&two), rows(&one));
        assert_eq!(rows_at(&two, Step::Normal), ["┌───┐", "│ A │", "└─┬─┘", "  ▾", "┌───┐", "│ B │", "└───┘"]);
    }

    #[test]
    fn circle_and_cross_heads_join_on_one_line() {
        let g = graph(
            Dir::LR,
            vec![rect("A"), rect("B"), rect("C")],
            vec![
                styled(0, 1, Stroke::Solid, Head::None, Head::Circle),
                styled(1, 0, Stroke::Solid, Head::None, Head::Cross),
                edge(0, 2, None),
            ],
        );
        assert_clean(&g);
        assert_eq!(rows_at(&g, Step::Normal)[..2], ["┌───┐    ┌───┐", "│ A │✕──o│ B │"]);
    }

    #[test]
    fn heads_that_clash_get_lines_of_their_own() {
        let g = graph(
            Dir::TD,
            vec![rect("A"), rect("B")],
            vec![edge(0, 1, None), styled(0, 1, Stroke::Solid, Head::None, Head::Circle)],
        );
        assert_clean(&g);
        let want = ["┌───┐", "│ A │", "└─┬─┘", "  │", " ┌┴┐", " ▾ o", "┌───┐", "│ B │", "└───┘"];
        assert_eq!(rows_at(&g, Step::Normal), want);
    }

    #[test]
    fn strokes_that_differ_get_lines_of_their_own() {
        let g = graph(
            Dir::TD,
            vec![rect("A"), rect("B")],
            vec![
                edge(0, 1, None),
                styled(0, 1, Stroke::Dotted, Head::None, Head::Arrow),
                styled(0, 1, Stroke::Thick, Head::None, Head::Arrow),
            ],
        );
        assert_clean(&g);
        let want = ["┌─────┐", "│  A  │", "└─┬┬┰─┘", "  │┆┃", "  ▾▾▾", "┌─────┐", "│  B  │", "└─────┘"];
        assert_eq!(rows_at(&g, Step::Normal), want);
    }

    #[test]
    fn lines_of_one_link_keep_their_order_at_both_ends() {
        // A's port for A --x B is shared with A --> C, which pulls it right;
        // at B the two lines must keep that order, or they would cross.
        let g = graph(
            Dir::TD,
            vec![rect("A"), rect("B"), rect("C")],
            vec![
                styled(0, 1, Stroke::Solid, Head::None, Head::Cross),
                styled(1, 0, Stroke::Solid, Head::Circle, Head::Arrow),
                edge(0, 2, None),
            ],
        );
        assert_clean(&g);
        let want = [
            "┌───┐",
            "│ A │",
            "└──┬┘",
            " ▴ │",
            " │ ├──────┐",
            " o ✕      ▾",
            "┌───┐   ┌───┐",
            "│ B │   │ C │",
            "└───┘   └───┘",
        ];
        assert_eq!(rows_at(&g, Step::Normal), want);
    }

    #[test]
    fn a_line_with_two_heads_never_shares_a_port() {
        // Through a shared port at P, Z's line would seem to lead to X.
        let g = graph(
            Dir::TD,
            vec![rect("X"), rect("Z"), rect("P")],
            vec![styled(0, 2, Stroke::Solid, Head::Arrow, Head::Arrow), edge(1, 2, None)],
        );
        assert_clean(&g);
        let want = [
            "┌───┐   ┌───┐",
            "│ X │   │ Z │",
            "└───┘   └─┬─┘",
            "  ▴       │",
            "  └──┐ ┌──┘",
            "     ▾ ▾",
            "    ┌───┐",
            "    │ P │",
            "    └───┘",
        ];
        assert_eq!(rows_at(&g, Step::Normal), want);
    }

    #[test]
    fn lanes_into_one_node_share_its_head() {
        // B --o A would put a circle where C → A's lane ends in an arrow, so
        // the back edges stay in the channels, each with its own head.
        let g = graph(
            Dir::TD,
            vec![rect("A"), rect("B"), rect("C")],
            vec![
                edge(0, 1, None),
                edge(1, 2, None),
                edge(2, 0, None),
                styled(1, 0, Stroke::Solid, Head::None, Head::Circle),
            ],
        );
        assert_clean(&g);
        let want = [
            "   ┌───┐",
            "   │ A │",
            "   └───┘",
            "    o ▴",
            "  ┌─┘ │",
            "  ▾   │",
            "┌───┐ │",
            "│ B │ │",
            "└─┬─┘ │",
            "  │   │",
            "  └┐ ┌┘",
            "   ▾ │",
            "  ┌──┴┐",
            "  │ C │",
            "  └───┘",
        ];
        assert_eq!(rows_at(&g, Step::Normal), want);
    }

    #[test]
    fn self_loops_keep_their_stroke_and_head() {
        let dotted = styled(0, 0, Stroke::Dotted, Head::None, Head::Arrow);
        let g = graph(Dir::TD, vec![rect("A"), rect("B")], vec![dotted.clone(), edge(0, 1, None)]);
        assert_clean(&g);
        let want = ["┌───┐", "│ A ├┄┐", "│   │◂┘", "└─┬─┘", "  ▾", "┌───┐", "│ B │", "└───┘"];
        assert_eq!(rows_at(&g, Step::Normal), want);
        let g = graph(Dir::LR, vec![rect("A"), rect("B")], vec![dotted, edge(0, 1, None)]);
        assert_clean(&g);
        let want = ["┌───┐    ┌───┐", "│ A ├───▸│ B │", "└┬──┘    └───┘", " ┆▴", " └┘"];
        assert_eq!(rows_at(&g, Step::Normal), want);
        let open = styled(0, 0, Stroke::Solid, Head::None, Head::None);
        let g = graph(Dir::TD, vec![rect("A"), rect("B")], vec![open, edge(0, 1, None)]);
        assert_clean(&g);
        let want = ["┌───┐", "│ A ├─┐", "│   ├─┘", "└─┬─┘", "  ▾", "┌───┐", "│ B │", "└───┘"];
        assert_eq!(rows_at(&g, Step::Normal), want);
    }

    #[test]
    fn self_loops_on_one_node_share_a_loop_or_are_dropped() {
        // Copies, one of them labelled, draw as one loop with the label.
        let g = graph(
            Dir::TD,
            vec![rect("A"), rect("B")],
            vec![edge(0, 0, None), edge(0, 0, Some("x")), edge(0, 0, None), edge(0, 1, None)],
        );
        assert_clean(&g);
        assert_eq!(rows_at(&g, Step::Normal)[..3], ["┌───┐", "│ A ├─┐ x", "│   │◂┘"]);
        // Two labels can't share one loop: every step fails, and the reader
        // gets the outline.
        let g = graph(
            Dir::TD,
            vec![rect("A"), rect("B")],
            vec![edge(0, 0, Some("one")), edge(0, 0, Some("two")), edge(0, 1, None)],
        );
        for (step, _, problems) in steps(&g) {
            assert_eq!(problems, Problems { dropped: 1, ..Problems::default() }, "{step:?}");
        }
        // Nor can two strokes.
        let g = graph(
            Dir::LR,
            vec![rect("A"), rect("B")],
            vec![edge(0, 0, None), styled(0, 0, Stroke::Dotted, Head::None, Head::Arrow), edge(0, 1, None)],
        );
        for (step, _, problems) in steps(&g) {
            assert_eq!(problems, Problems { dropped: 1, ..Problems::default() }, "{step:?}");
        }
    }

    #[test]
    fn edge_groups_in_random_graphs_draw_every_edge() {
        // Two nodes and many edges of every kind between them: every step
        // either passes its checks with every edge's heads drawn, or fails.
        for seed in 0..30u64 {
            let mut s = seed;
            let mut next = move || {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (s >> 33) as usize
            };
            let strokes = [Stroke::Solid, Stroke::Dotted, Stroke::Thick];
            let heads = [Head::None, Head::Arrow, Head::Circle, Head::Cross];
            let edges: Vec<Edge> = (0..2 + next() % 4)
                .map(|_| {
                    let (a, b) = if next() % 2 == 0 { (0, 1) } else { (1, 0) };
                    styled(a, b, strokes[next() % 3], heads[next() % 4], heads[next() % 4])
                })
                .collect();
            let dir = if seed % 2 == 0 { Dir::TD } else { Dir::LR };
            let g = graph(dir, vec![rect("A"), rect("B"), rect("C")], [edges, vec![edge(0, 2, None)]].concat());
            let mut work = Work::new(WORK_LIMIT);
            let p = prep::prepare(&g, &mut work).expect("prepared");
            let mut shown: Vec<usize> =
                p.link_lines.values().flatten().flat_map(|l| l.edges.iter().copied()).chain(p.lanes.clone()).collect();
            shown.sort_unstable();
            shown.dedup();
            assert_eq!(shown, (0..g.edges.len()).collect::<Vec<_>>(), "seed {seed}: every edge is on a line or a lane");
            for (step, rows, problems) in steps(&g) {
                if problems.any() {
                    continue;
                }
                let text = rows.concat();
                for e in &g.edges {
                    for h in [e.head_from, e.head_to] {
                        let glyphs: &[char] = match h {
                            Head::None => continue,
                            Head::Arrow => &['▴', '▾', '◂', '▸'],
                            Head::Circle => &['o'],
                            Head::Cross => &['✕'],
                        };
                        assert!(text.contains(glyphs), "seed {seed} {step:?}: {h:?} missing\n{}", rows.join("\n"));
                    }
                }
            }
        }
    }

    #[test]
    fn every_shape() {
        let shapes = [
            ("rect", Shape::Rect),
            ("round", Shape::Round),
            ("stadium", Shape::Stadium),
            ("sub", Shape::Subroutine),
            ("cyl", Shape::Cylinder),
            ("circ", Shape::Circle),
            ("dc", Shape::DCircle),
            ("dec", Shape::Decision),
            ("hex", Shape::Hexagon),
            ("lean", Shape::Slant),
            ("asym", Shape::Asym),
            ("bare", Shape::Bare),
        ];
        let nodes = shapes.iter().map(|&(l, s)| node(l, l, s)).collect();
        let edges = (0..shapes.len() - 1).map(|i| edge(i, i + 1, None)).collect();
        let g = graph(Dir::TD, nodes, edges);
        assert_clean(&g);
        let want = [
            " ┌───────┐",
            " │ rect  │",
            " └───┬───┘",
            "     ▾",
            " ╭───────╮",
            " │ round │",
            " ╰───┬───╯",
            "     ▾",
            "╭─────────╮",
            "( stadium )",
            "╰────┬────╯",
            "     ▾",
            " ┌┬─────┬┐",
            " ││ sub ││",
            " └┴──┬──┴┘",
            "     ▾",
            "  ╭─────╮",
            "  ├─────┤",
            "  │ cyl │",
            "  ╰──┬──╯",
            "     ▾",
            " ╭───────╮",
            " ( circ  )",
            " ╰───┬───╯",
            "     ▾",
            "  ╭─────╮",
            "  ( dc  )",
            "  ╰──┬──╯",
            "     ▾",
            "  ╱─────╲",
            "  < dec >",
            "  ╲──┬──╱",
            "     ▾",
            "  ╱─────╲",
            "  < hex >",
            "  ╲──┬──╱",
            "     ▾",
            " ┌───────┐",
            " / lean  /",
            " └───┬───┘",
            "     ▾",
            " ┌───────┐",
            " > asym  │",
            " └───┬───┘",
            "     ▾",
            "    bare",
        ];
        assert_eq!(rows_at(&g, Step::Normal), want);
    }

    #[test]
    fn cjk_label_breaks_between_wide_characters() {
        let g = graph(
            Dir::TD,
            vec![node("A", "認証サービスを呼び出す", Shape::Rect), node("B", "完了", Shape::Rect)],
            vec![edge(0, 1, None)],
        );
        assert_clean(&g);
        let want = [
            "┌───────────┐",
            "│ 認証サー  │",
            "│ ビスを呼  │",
            "│  び出す   │",
            "└─────┬─────┘",
            "      ▾",
            "  ┌───────┐",
            "  │ 完了  │",
            "  └───────┘",
        ];
        assert_eq!(rows_at(&g, Step::Labels8), want);
    }

    #[test]
    fn invisible_edges_only() {
        let mut e = edge(0, 1, None);
        e.stroke = Stroke::Invisible;
        e.head_to = Head::None;
        let g = graph(Dir::TD, vec![rect("A"), rect("B")], vec![e]);
        assert_clean(&g);
        assert_eq!(rows_at(&g, Step::Normal), ["┌───┐", "│ A │", "└───┘", "", "┌───┐", "│ B │", "└───┘"]);
    }

    #[test]
    fn node_reached_only_through_an_invisible_edge() {
        // The prototype crashed here: B's chain to C was never ordered.
        let mut inv = edge(0, 1, None);
        inv.stroke = Stroke::Invisible;
        let g = graph(Dir::TD, vec![rect("A"), rect("B"), rect("C")], vec![inv, edge(1, 2, Some("x"))]);
        assert_clean(&g);
    }

    #[test]
    fn empty_and_odd_labels() {
        let mut long = edge(1, 2, Some(""));
        long.min_len = 0;
        let g = graph(
            Dir::LR,
            vec![node("A", "", Shape::Rect), node("B", "", Shape::Bare), node("C", "🙂 é", Shape::Circle)],
            vec![edge(0, 1, None), long, edge(2, 2, None), edge(2, 2, Some("x"))],
        );
        for (_, rows, _) in steps(&g) {
            assert!(!rows.is_empty());
        }
        let g = graph(Dir::TD, vec![node("A", " ", Shape::Rect), rect("B")], vec![edge(0, 1, None)]);
        assert_eq!(rows_at(&g, Step::Normal), ["┌───┐", "│   │", "└─┬─┘", "  ▾", "┌───┐", "│ B │", "└───┘"]);
    }

    #[test]
    fn same_output_twice() {
        let g = graph(
            Dir::TD,
            (0..12).map(|i| rect(&format!("N{i}"))).collect(),
            (0..24).map(|i| edge(i % 12, (i * 7 + 3) % 12, (i % 5 == 0).then_some("label"))).collect(),
        );
        assert_eq!(steps(&g), steps(&g));
    }

    /// A deterministic pseudo-random graph.
    fn random_graph(seed: u64, n: usize, m: usize, dir: Dir) -> FlowGraph {
        let mut s = seed;
        let mut next = move || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 33) as usize
        };
        let shapes = [Shape::Rect, Shape::Round, Shape::Decision, Shape::Cylinder, Shape::Bare, Shape::Subroutine];
        let nodes = (0..n)
            .map(|i| node(&format!("n{i}"), &"word ".repeat(next() % 6), shapes[next() % shapes.len()]))
            .collect();
        let strokes = [Stroke::Solid, Stroke::Dotted, Stroke::Thick, Stroke::Invisible];
        let heads = [Head::None, Head::Arrow, Head::Circle, Head::Cross];
        let edges = (0..m)
            .map(|_| Edge {
                from: next() % n,
                to: next() % n,
                stroke: strokes[next() % strokes.len()],
                head_from: heads[next() % 4],
                head_to: heads[next() % 4],
                label: (next() % 3 == 0).then(|| "x y".to_string()),
                min_len: 1 + next() % 2,
                from_cluster: None,
                to_cluster: None,
            })
            .collect();
        graph(dir, nodes, edges)
    }

    #[test]
    fn large_and_random_graphs_never_panic() {
        for seed in 0..40 {
            let (n, m) = if seed < 4 { (150, 300) } else { (2 + seed as usize, 3 * seed as usize) };
            let dir = if seed % 2 == 0 { Dir::TD } else { Dir::LR };
            let g = random_graph(seed, n, m, dir);
            let mut work = Work::new(WORK_LIMIT);
            let Ok(p) = prep::prepare(&g, &mut work) else { continue };
            for &step in ladder(dir) {
                let _ = draw(&g, &p, step, Charset::Unicode, &mut work);
            }
        }
    }

    #[test]
    fn huge_minimum_length_is_too_large() {
        let mut e = edge(0, 1, None);
        e.min_len = usize::MAX;
        let g = graph(Dir::TD, vec![rect("A"), rect("B")], vec![e]);
        assert_eq!(prep::prepare(&g, &mut Work::new(WORK_LIMIT)).err(), Some(NotDrawn::TooLarge));
    }

    /// A flowchart from Mermaid source.
    fn parsed(src: &str) -> FlowGraph {
        let st = crate::diagram::pre::statements(src);
        crate::diagram::flow::parse::parse(&st[0], &st[1..]).expect("parses")
    }

    /// The first clean step and its rows, as the ladder would choose with
    /// no width limit.
    fn first_clean(g: &FlowGraph) -> Option<(Step, Vec<String>)> {
        steps(g).into_iter().find(|s| !s.2.any()).map(|s| (s.0, s.1))
    }

    #[test]
    fn a_lane_label_goes_past_the_lines_its_stub_crosses() {
        // C's label used to cover D → B's lane, which then seemed to run
        // into C's edge.
        let g = parsed("flowchart TD\nA --> B --> C --> D\nD --> B\nC -->|retry| A");
        assert_clean(&g);
        let rows = rows_at(&g, Step::Normal);
        assert_eq!(rows[5], "│ B │◂─┐         │");
        assert_eq!(rows[9], "│ C ├──│─ retry ─┘");
        assert_eq!(rows[13], "│ D ├──┘");
        let g = parsed("flowchart TD\nS --> F --> P --> V\nV -->|bad| F\nP -->|timeout| S");
        assert_clean(&g);
        assert_eq!(rows_at(&g, Step::Normal)[9], "│ P ├───────│─ timeout ─┘");
    }

    #[test]
    fn a_left_to_right_lane_label_stays_between_its_corners() {
        // Longer than the gap between A and B: the label used to run past
        // B's corner over B's fan-out and C2's border. The channel before B
        // now widens to hold it between the corners.
        let label = "send it all the way back to the start";
        let g = parsed(&format!(
            "flowchart LR\nA --> B\nB --> C1\nB --> C2\nB --> C3\nB --> C4\nB -->|{label}| A"
        ));
        assert_clean(&g);
        for (step, rows, _) in steps(&g) {
            let want = if step.turned() { format!("├─ {label} ─┘") } else { format!("└─ {label} ─┘") };
            assert!(rows.iter().any(|r| r.contains(&want)), "{step:?}\n{}", rows.join("\n"));
        }
        // It used to cover D → C's whole run, which then seemed to end at A.
        let g = parsed("flowchart LR\nA --> B\nB --> C\nC --> D\nD --> C\nB -->|a very long label for the loop back| A");
        assert_clean(&g);
        let rows = rows_at(&g, Step::Normal);
        assert!(rows[4].contains("└─ a very long label for the loop back ─┘"), "{}", rows.join("\n"));
        assert!(rows[4].ends_with("┘            └────────────┘"), "{}", rows.join("\n"));
        // A label that fits leaves the layout as it was.
        let g = parsed("flowchart LR\nA --> B --> C --> D\nD -->|again| A");
        assert_clean(&g);
        assert_eq!(rows_at(&g, Step::Normal)[4], "  └─ again ──────────────────────────────┘");
    }

    #[test]
    fn a_left_to_right_lane_label_moves_off_a_later_lane() {
        // W → P's rise (painted after V → F's run) crosses the run where
        // the label would start, so the label goes past it.
        let g = parsed("flowchart LR\nS --> F --> P --> V[Validate the input] --> W\nV -->|bad input data| F\nW --> P");
        assert_clean(&g);
        let rows = rows_at(&g, Step::Normal);
        let row = rows.iter().find(|r| r.contains("bad input data")).expect("label");
        let arrows: Vec<usize> =
            rows[3].char_indices().filter(|&(_, c)| c == '▴').map(|(b, _)| width(&rows[3][..b])).collect();
        let label_at = row.find(" bad").map(|b| width(&row[..b]));
        assert!(arrows.len() == 2 && label_at.is_some_and(|c| c > arrows[1]), "{}", rows.join("\n"));
        assert!(row.contains("──┘"), "{}", rows.join("\n"));
    }

    #[test]
    fn lanes_of_different_strokes_into_one_node_dont_share_a_line() {
        for src in [
            "flowchart TD\nA --> B --> C --> D\nC -.->|async refresh| A\nD -->|done| A",
            "flowchart TD\nA --> B --> C --> D\nC ==> A\nD --> A",
            "flowchart LR\nA --> B --> C --> D\nC -.-> A\nD --> A",
        ] {
            let g = parsed(src);
            let p = prep::prepare(&g, &mut Work::new(WORK_LIMIT)).unwrap();
            let strokes: Vec<Stroke> = p.lanes.iter().map(|&k| g.edges[k].stroke).collect();
            assert!(strokes.windows(2).all(|w| w[0] == w[1]), "{src}: {strokes:?}");
            assert!(first_clean(&g).is_some(), "{src}");
        }
        // The same stroke still shares one.
        let g = parsed("flowchart TD\nA --> B --> C --> D\nC -.-> A\nD -.-> A");
        assert_eq!(prep::prepare(&g, &mut Work::new(WORK_LIMIT)).unwrap().lanes.len(), 2);
    }

    #[test]
    fn a_labelled_branch_of_a_fan_out_gets_its_own_port() {
        // B's line used to join the x branch at one port on D, so B seemed
        // to reach C through A's fan-out.
        let g = parsed("flowchart TD\nA --> C\nA -->|x| D\nB --> D");
        assert_clean(&g);
        let want = [
            "  ┌───┐   ┌───┐",
            "  │ A │   │ B │",
            "  └─┬─┘   └─┬─┘",
            "    │       │",
            "    ├───┐   │",
            "    │   x   │",
            "    │   │   │",
            "  ┌─┘   └┐ ┌┘",
            "  ▾      ▾ ▾",
            "┌───┐   ┌───┐",
            "│ C │   │ D │",
            "└───┘   └───┘",
        ];
        assert_eq!(rows_at(&g, Step::Normal), want);
    }

    #[test]
    fn the_check_reads_lines_through_an_edge_label() {
        // Paint the old drawing: A's fan-out with a label on one branch,
        // whose line below the label joins B's line into D.
        let mut cv = Canvas::new(N | S);
        let own = |k| Some(Owner { bus: Bus::Segment { rank: 0, index: k }, edge: Some(k) });
        cv.owner = own(0);
        cv.line(0, 0, S, Stroke::Solid);
        cv.owner = own(1);
        cv.line(2, 0, N | E, Stroke::Solid);
        cv.owner = own(2);
        cv.line(2, 1, W, Stroke::Solid);
        cv.put_text(1, 0, "x", Role::EdgeLabel, false);
        // Edges: 0 is A → C (fan-out), 1 is A → D through the label, 2 is B → D.
        let edges = [Some((0, 2)), Some((0, 3)), Some((1, 3))];
        assert_eq!(check::flow_check(&cv, &edges, &[], &[], &mut Work::new(1000)), Ok(0));
        let joins = [((0, 0), (2, 0))];
        assert_eq!(check::flow_check(&cv, &edges, &[], &joins, &mut Work::new(1000)), Ok(1));
    }

    #[test]
    fn a_left_to_right_self_loop_label_keeps_clear_of_the_next_channel() {
        let g = parsed("flowchart LR\nA[Upload] -->|retry on failure| A\nA --> C[Store]\nB[Import] --> C");
        assert_clean(&g);
        let rows = rows_at(&g, Step::Normal);
        let row = rows.iter().find(|r| r.contains("retry on failure")).expect("label");
        let after = &row[row.find("failure").unwrap() + "failure".len()..];
        let blanks = after.chars().take_while(|&c| c == ' ').count();
        assert!(blanks >= 2 && after.len() > blanks, "{}", rows.join("\n"));
    }

    #[test]
    fn a_lane_without_a_head_ends_on_its_target_border() {
        let g = parsed("flowchart LR\nA --> B --> C\nC === A");
        assert_clean(&g);
        let want = [
            "┌───┐    ┌───┐    ┌───┐",
            "│ A ├───▸│ B ├───▸│ C │",
            "└─┰─┘    └───┘    └─┰─┘",
            "  ┃                 ┃",
            "  ┗━━━━━━━━━━━━━━━━━┛",
        ];
        assert_eq!(rows_at(&g, Step::Normal), want);
        let g = parsed("flowchart TD\nA --> B --> C\nC -.- A");
        assert_clean(&g);
        let rows = rows_at(&g, Step::Normal);
        assert_eq!((rows[1].as_str(), rows[9].as_str()), ("│ A ├┄┄┐", "│ C ├┄┄┘"));
    }

    #[test]
    fn a_line_with_two_heads_shows_its_stroke_between_them() {
        let g = parsed("flowchart LR\nA1[a long label one here] <--> B1[a long label two here] o--o C1[a long label three here] x--x D1[another long label four]");
        for (step, rows, problems) in steps(&g) {
            if !problems.any() && !step.turned() {
                let text = rows.concat();
                for pair in ["◂─", "─▸", "o─", "─o", "✕─", "─✕"] {
                    assert!(text.contains(pair), "{step:?}: {pair}\n{}", rows.join("\n"));
                }
                for pair in ["◂▸", "oo", "✕✕"] {
                    assert!(!text.contains(pair), "{step:?}: {pair}\n{}", rows.join("\n"));
                }
            }
        }
        assert_eq!(rows_at(&g, Step::LrLabels8)[3], "│  label   │◂─▸│  label   │o─o│ three  │✕─✕│  label  │");
        let g = parsed("flowchart TD\nA <--> B");
        assert_eq!(rows_at(&g, Step::Normal)[3..6], ["  ▴", "  │", "  ▾"]);
    }

    #[test]
    fn lines_touch_a_bare_node_text() {
        let g = parsed("flowchart LR\nA[Start] --> B@{ shape: text, label: \"done\" }");
        assert_clean(&g);
        assert_eq!(rows_at(&g, Step::Normal), ["┌───────┐", "│ Start ├───▸done", "└───────┘"]);
        let g = parsed("flowchart TD\nA@{ shape: text, label: \"x\" } --> B");
        assert_clean(&g);
        assert_eq!(rows_at(&g, Step::Normal), ["  x", "  ▾", "┌───┐", "│ B │", "└───┘"]);
        // No head: the line still ends next to the text, in both
        // directions, and lanes leave and enter beside it.
        let g = parsed("flowchart TD\nA@{ shape: text, label: \"hello\" } --- B@{ shape: text, label: \"world\" }\nB --> C@{ shape: text, label: \"end\" }\nC --> A");
        assert_clean(&g);
        assert_eq!(rows_at(&g, Step::Normal), ["hello◂┐", "  │   │", "world │", "  ▾   │", " end──┘"]);
        // More ports than the text has cells: the step fails rather than
        // put a port on a blank cell.
        let g = parsed("flowchart LR\nA[Start] --> B@{ shape: text, label: \"done\" }\nB --> C[Next]\nB -.-> D[Other]");
        for (step, _, problems) in steps(&g) {
            assert_eq!(problems.any(), !step.turned(), "{step:?}");
        }
        // A self-loop has no border to leave by.
        let g = parsed("flowchart TD\nA@{shape: text, label: \"Retry\"}\nA --> A\nA --> B");
        assert!(steps(&g).iter().all(|s| s.2.dropped > 0));
    }

    #[test]
    fn zero_width_characters_keep_the_box_square() {
        for src in ["flowchart TD\nA[\"ab&#8203;cd&#8203;ef\"] --> B[x]", "flowchart TD\nA[\"x&shy;y&shy;z\"] --> B[x]"] {
            let g = parsed(src);
            assert_clean(&g);
            let rows = rows_at(&g, Step::Normal);
            let w: Vec<usize> = rows[..3].iter().map(|r| width(r.trim_start())).collect();
            assert!(w.iter().all(|&x| x == w[0]), "{src}: {w:?}\n{}", rows.join("\n"));
        }
    }

    #[test]
    fn lines_keep_off_a_shape_s_inner_lines() {
        let g = parsed("flowchart TD\nA[(Queue)] -->|poll| A\nA --> B");
        assert_clean(&g);
        assert_eq!(rows_at(&g, Step::Normal)[..4], ["╭───────╮", "├───────┤", "│ Queue ├─┐ poll", "│       │◂┘"]);
        let g = parsed("flowchart LR\nA[(DB)] --> B\nA -.-> C");
        assert_clean(&g);
        let rows = rows_at(&g, Step::Normal);
        assert_eq!(rows[1], "├────┤    ┌───┐");
        assert!(rows[2].starts_with("│ DB ├───▸"));
        let g = parsed("flowchart LR\nB --> A[(DB)]\nC -.-> A");
        assert_clean(&g);
        assert!(!rows_at(&g, Step::Normal)[1].contains('▸'));
        let g = parsed("flowchart LR\nA[[Retry]] --> A\nA --> B");
        assert_clean(&g);
        assert_eq!(rows_at(&g, Step::Normal)[2..], ["└┴┬──────┴┘    └───┘", "  │▴", "  └┘"]);
        let g = parsed("flowchart TD\nA --> B[(DB)]\nB --> A");
        assert_clean(&g);
        assert_eq!(rows_at(&g, Step::Normal)[6], "│ DB  ├──┘");
    }

    #[test]
    fn the_check_follows_lines_as_drawn() {
        // X --- B is reversed to break the cycle and drawn from B, as
        // B --- X is: both draw, and draw the same.
        let a = parsed("flowchart TD\nB --> C\nC --> D\nD --> X\nX --- B\nX --> X");
        let b = parsed("flowchart TD\nB --> C\nC --> D\nD --> X\nB --- X\nX --> X");
        assert_clean(&a);
        let rows = |g: &FlowGraph| steps(g).into_iter().map(|s| s.1).collect::<Vec<_>>();
        assert_eq!(rows(&a), rows(&b));
    }

    /// The prototype's cycle breaking: search again from the start after
    /// each removal.
    fn break_cycles_slowly(above: &mut [BTreeMap<usize, i64>]) -> usize {
        let find = |above: &[BTreeMap<usize, i64>]| -> Option<Vec<usize>> {
            let m = above.len();
            let (mut color, mut parent) = (vec![0u8; m], vec![usize::MAX; m]);
            for s0 in 0..m {
                if color[s0] != 0 {
                    continue;
                }
                color[s0] = 1;
                let mut stack: Vec<(usize, Vec<usize>, usize)> = vec![(s0, above[s0].keys().copied().collect(), 0)];
                while let Some(top) = stack.last_mut() {
                    let u = top.0;
                    let mut next = None;
                    while top.2 < top.1.len() {
                        let v = top.1[top.2];
                        top.2 += 1;
                        if color[v] == 1 {
                            let (mut path, mut x) = (vec![v], u);
                            while x != v {
                                path.push(x);
                                x = parent[x];
                            }
                            path.reverse();
                            return Some(path);
                        }
                        if color[v] == 0 {
                            color[v] = 1;
                            parent[v] = u;
                            next = Some(v);
                            break;
                        }
                    }
                    match next {
                        Some(v) => stack.push((v, above[v].keys().copied().collect(), 0)),
                        None => {
                            color[u] = 2;
                            stack.pop();
                        }
                    }
                }
            }
            None
        };
        let mut hard = 0;
        while let Some(cyc) = find(above) {
            let mut best: Option<(usize, usize, i64)> = None;
            for i in 0..cyc.len() {
                let (a, b) = (cyc[i], cyc[(i + 1) % cyc.len()]);
                let w = above[a][&b];
                if best.is_none_or(|(_, _, bw)| w < bw) {
                    best = Some((a, b, w));
                }
            }
            let (a, b, w) = best.unwrap();
            hard += usize::from(w >= INF);
            above[a].remove(&b);
        }
        hard
    }

    #[test]
    fn cycle_breaking_goes_on_where_it_stands_and_matches_the_prototype() {
        for seed in 0..400u64 {
            let mut s = seed;
            let mut next = move || {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (s >> 33) as usize
            };
            let m = 2 + next() % 14;
            let mut above: Vec<BTreeMap<usize, i64>> = vec![BTreeMap::new(); m];
            for i in 0..m {
                for j in i + 1..m {
                    if next() % 3 == 0 {
                        continue;
                    }
                    let w = [0, 1, 2, 3, INF][next() % 5];
                    let (a, b) = if next() % 2 == 0 { (i, j) } else { (j, i) };
                    above[a].insert(b, w);
                }
            }
            let mut slow = above.clone();
            let hard_slow = break_cycles_slowly(&mut slow);
            let hard = break_cycles(&mut above, &mut Work::new(WORK_LIMIT)).unwrap();
            assert_eq!((hard, &above), (hard_slow, &slow), "seed {seed}");
        }
    }

    #[test]
    fn work_limit_stops_drawing() {
        let g = random_graph(7, 60, 120, Dir::TD);
        let mut work = Work::new(1000);
        let r = prep::prepare(&g, &mut work).and_then(|p| draw(&g, &p, Step::Normal, Charset::Unicode, &mut work));
        assert_eq!(r.err(), Some(NotDrawn::TooLarge));
    }
}
