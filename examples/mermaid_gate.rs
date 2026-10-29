//! The M2 corpus gate (docs/mermaid-plan.md, section 11): run folio's Mermaid
//! code over a sample of real diagrams and print the kill-criteria numbers.
//!
//! usage: cargo run --release --example mermaid_gate -- BLOCKS.jsonl [held|tune]
//!
//! BLOCKS.jsonl comes from docs/mermaid-prototype/proto2/gate_blocks.py. The
//! blocks are other people's content and stay outside the repository.

use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::time::Instant;

use folio::diagram::flow::{self, FlowGraph};
use folio::diagram::{self, Charset, Diagrams, MAX_EDGES, MAX_NODES, MAX_SOURCE, NotDrawn, Outcome, Request, Step, Work, pre};

const WIDTHS: [usize; 5] = [38, 51, 78, 100, 118];
const GATE_WIDTH: usize = 78;

#[derive(serde::Deserialize)]
struct Block {
    set: String,
    src: String,
}

/// What happened to one flowchart at one ladder step.
#[derive(Clone, Copy)]
struct StepResult {
    width: usize,
    ok: bool,
}

#[derive(Default)]
struct Stats {
    blocks: usize,
    crashes: usize,
    flow: usize,
    flow_source: usize,
    flow_caps: usize,
    flow_parsed: usize,
    flow_subgraphs: usize,
    /// Per width: drawn without subgraphs, drawn of all parsed.
    drawn_plain: [usize; WIDTHS.len()],
    too_large: usize,
    steps_at_gate: BTreeMap<String, usize>,
    fitting_drawings: usize,
    fitting_failed: usize,
    first_fit_failed: usize,
    problems: [usize; 5],
    seq: usize,
    seq_parsed: usize,
    other: usize,
    undetermined: usize,
    reasons: BTreeMap<String, usize>,
    times_ms: Vec<f64>,
    shown_too_wide: usize,
    outcomes: BTreeMap<&'static str, usize>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("usage: mermaid_gate BLOCKS.jsonl [held|tune]");
        std::process::exit(2);
    };
    let which = args.get(2).map_or("held", String::as_str);
    let text = std::fs::read_to_string(path).expect("can't read the blocks file");
    let blocks: Vec<Block> =
        text.lines().filter_map(|l| serde_json::from_str::<Block>(l).ok()).filter(|b| b.set == which).collect();
    // Count panics instead of printing each one.
    panic::set_hook(Box::new(|_| {}));
    let mut s = Stats::default();
    for b in &blocks {
        s.blocks += 1;
        if panic::catch_unwind(AssertUnwindSafe(|| measure(&b.src, &mut s))).is_err() {
            s.crashes += 1;
        }
        if panic::catch_unwind(AssertUnwindSafe(|| end_to_end(&b.src, &mut s))).is_err() {
            s.crashes += 1;
        }
    }
    report(&s, which);
}

fn keyword(src: &str) -> Option<String> {
    let (_, body) = pre::preamble(src).ok()?;
    let st = pre::statements(&body);
    st.first().and_then(|s| s.split_whitespace().next()).map(str::to_string)
}

fn measure(src: &str, s: &mut Stats) {
    let Some(kw) = keyword(src) else {
        s.undetermined += 1;
        return;
    };
    let (_, body) = pre::preamble(src).expect("checked above");
    let st = pre::statements(&body);
    match kw.as_str() {
        "flowchart" | "graph" | "flowchart-v2" | "flowchart-elk" => {
            s.flow += 1;
            let g = match flow::parse::parse(&st[0], &st[1..]) {
                Ok(g) => g,
                Err(e) => {
                    s.flow_source += 1;
                    let why = match e {
                        NotDrawn::Unsupported(w) => w,
                        NotDrawn::TooLarge => "too large".into(),
                    };
                    let category: String = why.split('"').next().unwrap_or("").trim().chars().take(40).collect();
                    *s.reasons.entry(category).or_default() += 1;
                    return;
                }
            };
            let visible = g.nodes.iter().filter(|n| !n.dead).count();
            if visible > MAX_NODES || g.edges.len() > MAX_EDGES || src.len() > MAX_SOURCE {
                s.flow_caps += 1;
                return;
            }
            s.flow_parsed += 1;
            if !g.clusters.is_empty() {
                s.flow_subgraphs += 1;
                return;
            }
            flowchart(&g, s);
        }
        "sequenceDiagram" => {
            s.seq += 1;
            if diagram::seq::parse::parse(&st[0], &st[1..]).is_ok() {
                s.seq_parsed += 1;
            }
        }
        _ => s.other += 1,
    }
}

/// Draw a flowchart without subgraphs at every ladder step and record which
/// step each width would show.
fn flowchart(g: &FlowGraph, s: &mut Stats) {
    let start = Instant::now();
    let mut work = Work::new(diagram::WORK_LIMIT);
    let mut results: Vec<(Step, StepResult)> = Vec::new();
    let mut too_large = false;
    match flow::prep::prepare(g, &mut work) {
        Ok(prep) => {
            for &step in diagram::ladder(g.dir) {
                match flow::draw::draw(g, &prep, step, Charset::Unicode, &mut work) {
                    Ok(c) => {
                        let p = c.problems;
                        for (i, n) in [p.text_over_line, p.false_pairs, p.bad_cells, p.hard_conflicts, p.dropped]
                            .into_iter()
                            .enumerate()
                        {
                            s.problems[i] += usize::from(n > 0);
                        }
                        results.push((step, StepResult { width: c.drawing.width, ok: !p.any() }));
                    }
                    Err(_) => {
                        too_large = true;
                        break;
                    }
                }
            }
        }
        Err(_) => too_large = true,
    }
    s.times_ms.push(start.elapsed().as_secs_f64() * 1000.0);
    s.too_large += usize::from(too_large);
    for (i, &w) in WIDTHS.iter().enumerate() {
        if results.iter().any(|(_, r)| r.width <= w && r.ok) {
            s.drawn_plain[i] += 1;
        }
    }
    let chosen = results.iter().find(|(_, r)| r.width <= GATE_WIDTH && r.ok);
    let name = chosen.map_or("outline".to_string(), |(step, _)| format!("{step:?}"));
    *s.steps_at_gate.entry(name).or_default() += 1;
    for (_, r) in &results {
        if r.width <= GATE_WIDTH {
            s.fitting_drawings += 1;
            s.fitting_failed += usize::from(!r.ok);
        }
    }
    if let Some((_, first)) = results.iter().find(|(_, r)| r.width <= GATE_WIDTH)
        && !first.ok
    {
        s.first_fit_failed += 1;
    }
}

/// Render through the public API twice with fresh caches: the outcome must
/// fit and be the same both times.
fn end_to_end(src: &str, s: &mut Stats) {
    let req = Request { width: GATE_WIDTH, charset: Charset::Unicode };
    let a = describe(diagram::render(src, &req, &mut Diagrams::default()));
    let b = describe(diagram::render(src, &req, &mut Diagrams::default()));
    assert_eq!(a, b, "output differs between two runs");
    *s.outcomes.entry(a.0).or_default() += 1;
    if a.1.lines().any(|l| pre::width(l) > GATE_WIDTH) {
        s.shown_too_wide += 1;
    }
}

fn describe(o: Outcome) -> (&'static str, String) {
    match o {
        Outcome::Drawn(d) => (if d.kind == diagram::Kind::Flowchart { "flowchart drawn" } else { "sequence drawn" }, d.text()),
        Outcome::Outline(d) => {
            (if d.kind == diagram::Kind::Flowchart { "flowchart outline" } else { "sequence transcript" }, d.text())
        }
        Outcome::Source(why) => ("source", why),
    }
}

fn pct(n: usize, d: usize) -> String {
    if d == 0 { "-".into() } else { format!("{:.1}%", 100.0 * n as f64 / d as f64) }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[i]
}

fn report(s: &Stats, which: &str) {
    let plain = s.flow_parsed - s.flow_subgraphs;
    println!("sample: {which}, {} blocks; crashes: {}", s.blocks, s.crashes);
    println!();
    println!("Flowcharts: {}", s.flow);
    println!("  to source {}, over caps {}, parsed {} ({}), with subgraphs {}", s.flow_source, s.flow_caps, s.flow_parsed, pct(s.flow_parsed, s.flow), s.flow_subgraphs);
    println!("  gate 1: parse rate {} (needs >= 95%)", pct(s.flow_parsed, s.flow));
    println!("  drawn by width (no subgraphs of {plain} | all parsed of {}, subgraphs as outlines):", s.flow_parsed);
    for (i, w) in WIDTHS.iter().enumerate() {
        println!("    {w:>3}: {:>6} | {:>6}", pct(s.drawn_plain[i], plain), pct(s.drawn_plain[i], s.flow_parsed));
    }
    let i78 = WIDTHS.iter().position(|&w| w == GATE_WIDTH).unwrap();
    println!(
        "  gate 2 at {GATE_WIDTH}: {} without subgraphs (needs >= 88%), {} of all parsed (needs >= 75%)",
        pct(s.drawn_plain[i78], plain),
        pct(s.drawn_plain[i78], s.flow_parsed)
    );
    println!(
        "  gate 3: fitting drawings that fail a check {} of {} ({}; needs < 0.5%); first fitting step failed for {} diagrams",
        s.fitting_failed,
        s.fitting_drawings,
        pct(s.fitting_failed, s.fitting_drawings),
        s.first_fit_failed
    );
    println!(
        "  diagrams with a step failing: text over line {}, false pairs {}, bad cells {}, hard conflicts {}, dropped {}",
        s.problems[0], s.problems[1], s.problems[2], s.problems[3], s.problems[4]
    );
    println!("  too large (work limit or caps after preparation): {}", s.too_large);
    println!("  steps used at {GATE_WIDTH}: {:?}", s.steps_at_gate);
    let mut t = s.times_ms.clone();
    t.sort_by(f64::total_cmp);
    println!(
        "  gate 4: time for all ladder steps, ms: p50 {:.2}, p90 {:.2}, p99 {:.2} (needs <= 20), max {:.2}",
        percentile(&t, 0.5),
        percentile(&t, 0.9),
        percentile(&t, 0.99),
        t.last().copied().unwrap_or(0.0)
    );
    println!("  source reasons: {:?}", s.reasons);
    println!();
    println!("Sequence diagrams: {}, parsed {} ({})", s.seq, s.seq_parsed, pct(s.seq_parsed, s.seq));
    println!("Other types: {}; no keyword found: {}", s.other, s.undetermined);
    println!();
    println!("End to end at {GATE_WIDTH} (render twice, fresh cache): {:?}", s.outcomes);
    println!("  shown output wider than {GATE_WIDTH}: {}", s.shown_too_wide);
}
