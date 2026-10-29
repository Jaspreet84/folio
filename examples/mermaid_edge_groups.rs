//! Count flowcharts whose edges share a drawn line: edges joining the same
//! two nodes on neighbouring ranks (in either direction), lines with heads
//! at both ends, self-loops, and lanes. Each kind is counted with how the
//! drawing at 78 columns turned out.
//!
//! usage: cargo run --release --example mermaid_edge_groups -- BLOCKS.jsonl [held|tune] [--show KIND N]
//!
//! BLOCKS.jsonl comes from docs/mermaid-prototype/proto2/gate_blocks.py.
//! `--show KIND N` prints the first N diagrams of a kind whose name
//! contains KIND, drawn at the step the gate would choose.

use std::collections::{BTreeMap, HashMap};

use folio::diagram::flow::{self, FlowGraph, Head, Stroke, draw, prep};
use folio::diagram::{self, Charset, MAX_EDGES, MAX_NODES, MAX_SOURCE, Problems, WORK_LIMIT, Work, pre};

const GATE_WIDTH: usize = 78;

#[derive(serde::Deserialize)]
struct Block {
    set: String,
    src: String,
}

/// How one diagram turned out at the gate width.
fn at_gate(g: &FlowGraph, p: &prep::Prepared) -> (String, Option<String>) {
    let mut work = Work::new(WORK_LIMIT);
    let mut failed = Problems::default();
    for &step in diagram::ladder(g.dir) {
        let Ok(c) = draw::draw(g, p, step, Charset::Unicode, &mut work) else { return ("too large".into(), None) };
        if c.drawing.width <= GATE_WIDTH {
            if !c.problems.any() {
                return ("drawn".into(), Some(format!("{step:?}\n{}", c.drawing.text())));
            }
            failed = c.problems;
        }
    }
    let why = if failed.dropped > 0 {
        "outline (dropped)"
    } else if failed.any() {
        "outline (other checks)"
    } else {
        "outline (too wide)"
    };
    (why.into(), None)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("usage: mermaid_edge_groups BLOCKS.jsonl [held|tune] [--show KIND N]");
        std::process::exit(2);
    };
    let which = args.get(2).map_or("held", String::as_str);
    let show_at = args.iter().position(|a| a == "--show");
    let show_kind = show_at.and_then(|i| args.get(i + 1)).cloned().unwrap_or_default();
    let mut show: usize = show_at.and_then(|i| args.get(i + 2)).and_then(|s| s.parse().ok()).unwrap_or(0);
    let text = std::fs::read_to_string(path).expect("can't read the blocks file");
    let mut plain = 0;
    // Kind → outcome at 78 → diagrams.
    let mut kinds: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
    let mut edges_in_groups = 0;
    for line in text.lines() {
        let Ok(b) = serde_json::from_str::<Block>(line) else { continue };
        if b.set != which {
            continue;
        }
        let Ok((_, body)) = pre::preamble(&b.src) else { continue };
        let st = pre::statements(&body);
        let Some(kw) = st.first().and_then(|s| s.split_whitespace().next()) else { continue };
        if !matches!(kw, "flowchart" | "graph" | "flowchart-v2" | "flowchart-elk") {
            continue;
        }
        let Ok(g) = flow::parse::parse(&st[0], &st[1..]) else { continue };
        let visible = g.nodes.iter().filter(|n| !n.dead).count();
        if visible > MAX_NODES || g.edges.len() > MAX_EDGES || b.src.len() > MAX_SOURCE || !g.clusters.is_empty() {
            continue;
        }
        plain += 1;
        let Ok(p) = prep::prepare(&g, &mut Work::new(WORK_LIMIT)) else { continue };

        let mut found: Vec<&str> = Vec::new();
        // Links with more than one edge.
        let lines = &p.link_lines;
        let groups: Vec<&Vec<prep::Line>> =
            lines.values().filter(|ls| ls.iter().map(|l| l.edges.len()).sum::<usize>() > 1).collect();
        if !groups.is_empty() {
            found.push("link group (any)");
            edges_in_groups += groups.iter().map(|ls| ls.iter().map(|l| l.edges.len()).sum::<usize>()).sum::<usize>();
            if groups.iter().any(|ls| ls.len() > 1) {
                found.push("link group: separate lines needed");
            } else {
                found.push("link group: one line each");
            }
            let same = |l: &prep::Line| {
                l.edges.windows(2).all(|w| {
                    let (a, b) = (&g.edges[w[0]], &g.edges[w[1]]);
                    (a.from, a.to, a.head_from, a.head_to) == (b.from, b.to, b.head_from, b.head_to)
                })
            };
            if groups.iter().all(|ls| ls.len() == 1 && same(&ls[0])) {
                found.push("link group: identical duplicates only");
            }
            let opposite = groups.iter().any(|ls| {
                ls.iter().flat_map(|l| &l.edges).any(|&k| p.back[k])
                    && ls.iter().flat_map(|l| &l.edges).any(|&k| !p.back[k])
            });
            if opposite {
                found.push("link group: opposite directions");
            }
        } else if lines.values().flatten().any(|l| l.both_ways()) {
            found.push("line with heads at both ends, no link group");
        }
        // Several self-loops on one node.
        let mut loops: HashMap<usize, Vec<usize>> = HashMap::new();
        for &k in &p.selfloops {
            loops.entry(g.edges[k].from).or_default().push(k);
        }
        if loops.values().any(|ks| ks.len() > 1) {
            found.push("self-loops: several on one node");
        }
        let one_loop = |ks: &Vec<usize>| {
            let e0 = &g.edges[ks[0]];
            let mut labels: Vec<&str> =
                ks.iter().filter_map(|&k| g.edges[k].label.as_deref()).filter(|l| !l.is_empty()).collect();
            labels.dedup();
            ks.iter().all(|&k| {
                let e = &g.edges[k];
                e.stroke == e0.stroke && e.head_from == Head::None && (e.head_to == e0.head_to)
            }) && labels.len() <= 1
        };
        if loops.values().any(|ks| ks.len() > 1 && !one_loop(ks)) {
            found.push("self-loops: several on one node, not one loop");
        }
        if p.selfloops.iter().any(|&k| g.edges[k].stroke != Stroke::Solid || g.edges[k].head_to != Head::Arrow) {
            found.push("self-loop: not a solid arrow");
        }
        if p.selfloops.iter().any(|&k| g.edges[k].head_from != Head::None) {
            found.push("self-loop: head at its source");
        }
        // Lanes: a head at the source, or lanes into one node that differ.
        if p.lanes.iter().any(|&k| g.edges[k].head_from != Head::None) {
            found.push("lane: head at its source");
        }
        let mut into: HashMap<usize, Vec<usize>> = HashMap::new();
        for &k in &p.lanes {
            into.entry(g.edges[k].to).or_default().push(k);
        }
        if into.values().any(|ks| ks.iter().any(|&k| g.edges[k].stroke != g.edges[ks[0]].stroke)) {
            found.push("lanes: strokes differ into one node");
        }
        if into.values().any(|ks| ks.iter().any(|&k| g.edges[k].head_to != g.edges[ks[0]].head_to)) {
            found.push("lanes: heads differ into one node");
        }
        if found.is_empty() {
            continue;
        }
        let (outcome, drawing) = at_gate(&g, &p);
        for f in found.iter().copied() {
            *kinds.entry(f).or_default().entry(outcome.clone()).or_default() += 1;
        }
        if show > 0 && found.iter().any(|f| f.contains(show_kind.as_str())) {
            show -= 1;
            println!("---- {outcome}\n{}\n{}", b.src.trim(), drawing.unwrap_or_default());
        }
    }
    println!("{which}: {plain} parsed flowcharts without subgraphs; {edges_in_groups} edges in link groups");
    for (kind, outcomes) in &kinds {
        let n: usize = outcomes.values().sum();
        println!("  {n:5}  {kind}: {outcomes:?}");
    }
}
