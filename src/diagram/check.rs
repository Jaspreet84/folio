//! Correctness checks on a painted canvas (plan section 6.2 M): text over
//! lines, cells shared by two buses, and false connections. Ported from
//! `check.py` in the prototype.

use std::collections::{HashMap, HashSet};

use super::canvas::{Canvas, E, N, S, W};
use super::{NotDrawn, Work};

/// Label text painted over a line where it wasn't meant to be.
pub fn text_over_line(cv: &Canvas) -> usize {
    cv.text_over_line()
}

/// Cells shared by two different buses: (clean right-angle crossings,
/// anything else).
pub fn cell_check(cv: &Canvas, work: &mut Work) -> Result<(usize, usize), NotDrawn> {
    let (mut crossings, mut bad) = (0, 0);
    // Every cell is looked at; grouping a shared cell's owners compares
    // each with the ones before it.
    work.spend(cv.len() as u64)?;
    for (owners, gs) in cv.shared_cells() {
        work.spend((owners * owners) as u64)?;
        if gs.len() < 2 {
            continue;
        }
        let clean = gs.len() == 2 && {
            let (a, b) = (gs[0].bits, gs[1].bits);
            (a == N | S && b == W | E) || (a == W | E && b == N | S)
        };
        if clean {
            crossings += 1;
        } else {
            bad += 1;
        }
    }
    Ok((crossings, bad))
}

/// Two cells, as (y, x), whose lines a reader reads as one.
pub type Join = ((i64, i64), (i64, i64));

/// Trace the visible lines cell to cell into nets, and count the pairs of
/// nodes a net seems to connect that no edge joins. `edges[k]` is
/// `Some((upper, lower))` for each visible edge: the ends a reader traces
/// it from and to. `also[k]` lists the other edges that edge `k`'s lines
/// stand for (edges sharing one line); a net with edge `k` in it holds
/// those too. `joins` are pairs of cells a reader reads straight through,
/// such as the line ends above and below an edge label: their nets are one.
pub fn flow_check(
    cv: &Canvas,
    edges: &[Option<(usize, usize)>],
    also: &[Vec<usize>],
    joins: &[Join],
    work: &mut Work,
) -> Result<usize, NotDrawn> {
    // Each cell's visible bits, its two neighbours and its owners' edges.
    let owners: usize = (0..cv.len()).map(|i| cv.owner_count(i)).sum();
    work.spend(4 * cv.len() as u64 + 2 * owners as u64)?;
    // Visible bits of every cell an owner drew in.
    let vis: Vec<Option<u8>> = (0..cv.len()).map(|i| cv.owned_vis(i)).collect();
    let mut par: Vec<usize> = (0..cv.len()).collect();
    fn find(par: &mut [usize], mut i: usize) -> usize {
        while par[i] != i {
            par[i] = par[par[i]];
            i = par[i];
        }
        i
    }
    for (i, &(y, x)) in cv.positions().iter().enumerate() {
        let Some(b) = vis[i] else { continue };
        for (bit, dy, dx, opp) in [(E, 0, 1, W), (S, 1, 0, N)] {
            if b & bit == 0 {
                continue;
            }
            if let Some(j) = cv.index(y + dy, x + dx)
                && vis[j].is_some_and(|v| v & opp != 0)
            {
                let (ri, rj) = (find(&mut par, i), find(&mut par, j));
                par[ri] = rj;
            }
        }
    }
    work.spend(joins.len() as u64)?;
    for &((ya, xa), (yb, xb)) in joins {
        if let (Some(i), Some(j)) = (cv.index(ya, xa), cv.index(yb, xb))
            && vis[i].is_some()
            && vis[j].is_some()
        {
            let (ri, rj) = (find(&mut par, i), find(&mut par, j));
            par[ri] = rj;
        }
    }
    // The edges of each net, by the net's first cell.
    let mut net_of: HashMap<usize, usize> = HashMap::new();
    let mut nets: Vec<Vec<usize>> = Vec::new();
    for (i, v) in vis.iter().enumerate() {
        let Some(v) = *v else { continue };
        let root = find(&mut par, i);
        let n = *net_of.entry(root).or_insert_with(|| {
            nets.push(Vec::new());
            nets.len() - 1
        });
        nets[n].extend(cv.edges_with(i, v).filter(|&k| edges.get(k).is_some_and(|e| e.is_some())));
    }
    let eset: HashSet<(usize, usize)> = edges.iter().flatten().copied().collect();
    let mut false_pairs = 0;
    for ks in &mut nets {
        ks.sort_unstable();
        ks.dedup();
        let more: Vec<usize> = ks
            .iter()
            .flat_map(|&k| also.get(k).into_iter().flatten().copied())
            .filter(|&k| edges.get(k).is_some_and(|e| e.is_some()))
            .collect();
        if !more.is_empty() {
            ks.extend(more);
            ks.sort_unstable();
            ks.dedup();
        }
        let mut srcs: Vec<usize> = ks.iter().filter_map(|&k| edges[k].map(|e| e.0)).collect();
        let mut dsts: Vec<usize> = ks.iter().filter_map(|&k| edges[k].map(|e| e.1)).collect();
        srcs.sort_unstable();
        srcs.dedup();
        dsts.sort_unstable();
        dsts.dedup();
        work.spend((srcs.len() * dsts.len()) as u64)?;
        for &s in &srcs {
            for &t in &dsts {
                if s != t && !eset.contains(&(s, t)) && !eset.contains(&(t, s)) {
                    false_pairs += 1;
                }
            }
        }
    }
    Ok(false_pairs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::canvas::{Bus, Owner};
    use crate::diagram::flow::Stroke;

    fn owner(n: usize) -> Option<Owner> {
        Some(Owner { bus: Bus::Lane(n), edge: Some(n) })
    }

    #[test]
    fn crossings_are_clean_and_overlaps_are_not() {
        let mut cv = Canvas::new(N | S);
        cv.owner = owner(0);
        cv.line(0, 0, N | S, Stroke::Solid);
        cv.line(1, 0, N | S, Stroke::Solid);
        cv.owner = owner(1);
        cv.line(0, 0, W | E, Stroke::Solid);
        cv.line(1, 0, N | S, Stroke::Solid);
        assert_eq!(cell_check(&cv, &mut Work::new(1000)), Ok((1, 1)));
    }

    #[test]
    fn a_shared_line_joins_unrelated_nodes() {
        // Edge 0 is 0 → 2 and edge 1 is 1 → 3; their lines touch, so the
        // drawing seems to join 0 to 3 and 1 to 2.
        let mut cv = Canvas::new(N | S);
        cv.owner = owner(0);
        cv.line(0, 0, E, Stroke::Solid);
        cv.owner = owner(1);
        cv.line(0, 1, W, Stroke::Solid);
        let edges = [Some((0, 2)), Some((1, 3))];
        assert_eq!(flow_check(&cv, &edges, &[], &[], &mut Work::new(1000)), Ok(2));
        // A clean crossing joins nothing.
        let mut cv = Canvas::new(N | S);
        cv.owner = owner(0);
        cv.line(0, 1, N | S, Stroke::Solid);
        cv.owner = owner(1);
        for x in 0..3 {
            cv.line(0, x, W | E, Stroke::Solid);
        }
        assert_eq!(flow_check(&cv, &edges, &[], &[], &mut Work::new(1000)), Ok(0));
    }

    #[test]
    fn a_shared_line_counts_every_edge_it_stands_for() {
        // Edge 0 (0 → 1) draws a line that also stands for edge 2 (1 → 0),
        // and edge 1 (2 → 1) joins it. Edge 0 alone pairs only edges' own
        // ends; with edge 2, the net seems to lead from 2 to 0 as well.
        let mut cv = Canvas::new(N | S);
        cv.owner = owner(0);
        cv.line(0, 0, E, Stroke::Solid);
        cv.owner = owner(1);
        cv.line(0, 1, W, Stroke::Solid);
        let edges = [Some((0, 1)), Some((2, 1)), Some((1, 0))];
        assert_eq!(flow_check(&cv, &edges, &[], &[], &mut Work::new(1000)), Ok(0));
        let also = [vec![2], Vec::new(), Vec::new()];
        assert_eq!(flow_check(&cv, &edges, &also, &[], &mut Work::new(1000)), Ok(1));
    }
}
