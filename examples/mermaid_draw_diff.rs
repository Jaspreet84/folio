//! Differential test of the flowchart drawing engine against the Python
//! prototype (Mermaid plan, milestone M2).
//!
//! `docs/mermaid-prototype/proto2/m2_dump.py` writes one JSON line per
//! flowchart: the graph in the IR's serde shape and the prototype's rows and
//! metrics at every ladder step. This program prepares and draws each graph
//! at the same steps and compares the rows and the five problem counts.
//!
//! usage: cargo run --release --example mermaid_draw_diff -- DUMP.jsonl [options]
//!
//! - `--out DIR`: write a few small examples of each kind of mismatch to
//!   DIR/mismatches.txt, and every mismatching step to DIR/mismatch_list.txt.
//! - `--lr-loops`: draw left-to-right self-loops (the prototype drops them),
//!   and report what that does at the steps the prototype failed for them.
//! - `--repeat N`: time each diagram as the fastest of N runs.
//! - `--show HASH`: print the drawing of that diagram at every step.
//! - `--profile`: time painting, checks and rendering separately.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader};
use std::time::Instant;

use unicode_segmentation::UnicodeSegmentation;

use folio::diagram::flow::{FlowGraph, Head, Stroke, draw, prep};
use folio::diagram::{Charset, NotDrawn, Problems, Step, WORK_LIMIT, Work, check, pre};

fn step_of(name: &str) -> Option<Step> {
    Some(match name {
        "N" => Step::Normal,
        "C" => Step::Compact,
        "L16lr" => Step::LrLabels16,
        "L8lr" => Step::LrLabels8,
        "T" => Step::Turned,
        "TC" => Step::TurnedCompact,
        "L16" => Step::Labels16,
        "L8" => Step::Labels8,
        _ => return None,
    })
}

/// A mismatch kept as an example: (size, text).
type Example = (usize, String);

#[derive(Default)]
struct Tally {
    diagrams: usize,
    by_set: BTreeMap<String, usize>,
    steps: usize,
    steps_match: usize,
    /// By set: (steps compared, identical).
    steps_by_set: BTreeMap<String, (usize, usize)>,
    /// Kind of mismatch → count.
    kinds: BTreeMap<String, usize>,
    examples: BTreeMap<String, Vec<Example>>,
    times: Vec<f64>,
    times_by_set: BTreeMap<String, Vec<f64>>,
    /// Steps drawn the same way twice.
    deterministic: usize,
    too_large_prep: usize,
    too_large_draw: usize,
    py_prep_crash: usize,
    py_step_crash: usize,
    /// Prototype crash cases: what Rust did.
    crash_outcomes: BTreeMap<String, usize>,
    width_diff_diagrams: usize,
    seg_diff_diagrams: usize,
    /// Diagrams by whether edges share a link (see `group` in `main`), and
    /// how many of each differ from the prototype for another reason than
    /// label widths or graphemes.
    group_diagrams: BTreeMap<&'static str, usize>,
    group_differ: BTreeMap<&'static str, usize>,
    /// One line per mismatching step.
    all: String,
    /// With `--lr-loops`: what happened at steps the prototype failed for a
    /// dropped self-loop.
    lr_loop_steps: BTreeMap<String, usize>,
    /// (milliseconds, description) of the slowest diagrams.
    slowest: Vec<(f64, String)>,
}

impl Tally {
    fn note(&mut self, kind: &str, size: usize, text: impl FnOnce() -> String) {
        *self.kinds.entry(kind.to_string()).or_default() += 1;
        let ex = self.examples.entry(kind.to_string()).or_default();
        if ex.len() < 5 || ex.iter().any(|(s, _)| *s > size) {
            ex.push((size, text()));
            ex.sort_by_key(|(s, _)| *s);
            ex.truncate(5);
        }
    }
}

fn problems_of(m: &[serde_json::Value]) -> Problems {
    let n = |i: usize| m.get(i).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    Problems { text_over_line: n(2), false_pairs: n(3), bad_cells: n(5), hard_conflicts: n(6), dropped: n(7) }
}

fn quantile(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v[((p * v.len() as f64) as usize).min(v.len() - 1)]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().expect("usage: mermaid_draw_diff DUMP.jsonl [--out DIR] [--lr-loops]");
    let out_dir = args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)).cloned();
    let lr_loops = args.iter().any(|a| a == "--lr-loops");
    let profile = args.iter().any(|a| a == "--profile");
    // Timing takes the fastest of this many runs per diagram.
    let repeat: usize = args
        .iter()
        .position(|a| a == "--repeat")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    // Diagrams (by hash) whose drawings to print at every step.
    let show: Vec<String> = args.windows(2).filter(|w| w[0] == "--show").map(|w| w[1].clone()).collect();
    let file = std::fs::File::open(path).expect("open dump");
    let mut t = Tally::default();
    for line in BufReader::new(file).lines() {
        let line = line.expect("read dump");
        let rec: serde_json::Value = serde_json::from_str(&line).expect("parse dump line");
        let set = rec["set"].as_str().unwrap_or("?").to_string();
        let hash = rec["h"].as_str().unwrap_or("?").to_string();
        let mut g: FlowGraph = serde_json::from_value(rec["graph"].clone()).expect("graph");
        g.ids = g.nodes.iter().enumerate().map(|(i, n)| (n.id.clone(), i)).collect();
        t.diagrams += 1;
        *t.by_set.entry(set.clone()).or_default() += 1;
        let size = g.nodes.len() + g.edges.len();

        // Labels whose width differs between Python's table and unicode-width.
        let labels: Vec<String> = g
            .nodes
            .iter()
            .map(|n| n.label.replace('\n', " "))
            .chain(g.edges.iter().map(|e| e.label.clone().unwrap_or_default().replace('\n', " ")))
            .collect();
        let pw: Vec<u64> =
            rec["pw"].as_array().map(|a| a.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
        let width_diff = labels.iter().zip(&pw).any(|(l, &w)| pre::width(l) as u64 != w);
        if width_diff {
            t.width_diff_diagrams += 1;
        }
        // Grapheme widths: the prototype joins only zero-width characters to
        // the one before; unicode-segmentation also joins spacing marks.
        let pg: Vec<Vec<u64>> = rec["pg"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|l| l.as_array().map(|x| x.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default();
        let seg_diff = !width_diff
            && labels.iter().zip(&pg).any(|(l, p)| {
                let ours: Vec<u64> = l.graphemes(true).map(|g| pre::width(g) as u64).collect();
                ours != *p
            });
        if seg_diff {
            t.seg_diff_diagrams += 1;
        }

        let mut work = Work::new(WORK_LIMIT);
        let prepared = prep::prepare(&g, &mut work);
        if let Some(why) = rec.get("prep_crash").and_then(|v| v.as_str()) {
            t.py_prep_crash += 1;
            let mut outcome = match &prepared {
                Ok(_) => "prepared".to_string(),
                Err(e) => format!("{e:?}"),
            };
            if let Ok(p) = &prepared {
                let base = g.dir.base();
                let mut drawn = 0;
                let mut clean = 0;
                for &step in folio::diagram::ladder(base) {
                    if let Ok(c) = draw::draw_with(&g, p, step, Charset::Unicode, lr_loops, &mut work) {
                        drawn += 1;
                        if !c.problems.any() {
                            clean += 1;
                        }
                    }
                }
                outcome = format!("prepared, {drawn} steps drawn, {clean} without problems");
            }
            *t.crash_outcomes
                .entry(format!("prep crash ({}) -> {}", why.split(' ').next().unwrap_or(""), outcome))
                .or_default() += 1;
            t.note("python prepare crashed", size, || format!("{set} {hash}: {why}\n{}", graph_text(&g)));
            continue;
        }
        let prepared = match prepared {
            Ok(p) => p,
            Err(e) => {
                if e == NotDrawn::TooLarge {
                    t.too_large_prep += 1;
                }
                let _ = writeln!(
                    t.all,
                    "{set} {hash} prepare {e:?}: python widest {} elements {}",
                    rec["widest"], rec["elems"]
                );
                t.note("rust prepare: too large", size, || {
                    format!("{set} {hash}: python widest {} elems {}", rec["widest"], rec["elems"])
                });
                continue;
            }
        };
        if profile {
            profile_phases(&g, &prepared, &hash);
        }
        if show.contains(&hash) {
            for &step in folio::diagram::ladder(g.dir) {
                let mut w = Work::new(WORK_LIMIT);
                if let Ok(c) = draw::draw_with(&g, &prepared, step, Charset::Unicode, lr_loops, &mut w) {
                    println!("{hash} {step:?} {:?}\n{}", c.problems, c.drawing.text());
                }
            }
        }
        // Diagrams the prototype draws differently on purpose: edges sharing
        // a link (one line, or lines side by side), lines with heads at both
        // ends (ports of their own), self-loops other than one plain solid
        // arrow (drawn in their stroke and head, or dropped), and back edges
        // a lane can't show (a head at the source, or another head than the
        // other lanes into the same node).
        let lines: Vec<&prep::Line> = prepared.link_lines.values().flatten().collect();
        let loops_differ = prepared.selfloops.iter().any(|&k| {
            let e = &g.edges[k];
            let other_label =
                prepared.selfloops.iter().any(|&j| g.edges[j].from == e.from && g.edges[j].label != e.label);
            e.stroke != Stroke::Solid || e.head_from != Head::None || e.head_to != Head::Arrow || other_label
        });
        let back: Vec<usize> = (0..g.edges.len()).filter(|&k| prepared.back[k]).collect();
        let lanes_differ = back.iter().any(|&k| {
            let e = &g.edges[k];
            e.head_from != Head::None || back.iter().any(|&j| g.edges[j].to == e.to && g.edges[j].head_to != e.head_to)
        });
        let group = if prepared.link_lines.values().any(|ls| ls.iter().map(|l| l.edges.len()).sum::<usize>() > 1) {
            "edges sharing a link"
        } else if lines.iter().any(|l| l.both_ways()) {
            "a line with heads at both ends, no shared link"
        } else if loops_differ {
            "a self-loop other than one plain solid arrow"
        } else if lanes_differ {
            "a back edge with a head at its source or another head into its node"
        } else {
            "none of these"
        };
        *t.group_diagrams.entry(group).or_default() += 1;
        let mut differs = false;
        let steps = rec["steps"].as_object().cloned().unwrap_or_default();
        for (name, s) in &steps {
            let Some(step) = step_of(name) else { continue };
            t.steps += 1;
            t.steps_by_set.entry(set.clone()).or_default().0 += 1;
            let result = draw::draw_with(&g, &prepared, step, Charset::Unicode, lr_loops, &mut work);
            if let Some(why) = s.get("crash").and_then(|v| v.as_str()) {
                t.py_step_crash += 1;
                let outcome = match &result {
                    Ok(c) if c.problems.any() => "drawn with problems",
                    Ok(_) => "drawn without problems",
                    Err(_) => "not drawn",
                };
                *t.crash_outcomes
                    .entry(format!("step crash ({}) -> {outcome}", why.split(' ').next().unwrap_or("")))
                    .or_default() += 1;
                t.note("python step crashed", size, || format!("{set} {hash} {name}: {why}\n{}", graph_text(&g)));
                continue;
            }
            let c = match result {
                Ok(c) => c,
                Err(e) => {
                    if e == NotDrawn::TooLarge {
                        t.too_large_draw += 1;
                    }
                    t.note("rust draw: too large", size, || format!("{set} {hash} {name}"));
                    continue;
                }
            };
            let py_rows: Vec<String> = s["rows"]
                .as_array()
                .map(|a| a.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect())
                .unwrap_or_default();
            let rs_rows: Vec<String> = c.drawing.text().lines().map(str::to_string).collect();
            // Determinism: prepare and draw again from scratch.
            let mut w2 = Work::new(WORK_LIMIT);
            let again = prep::prepare(&g, &mut w2)
                .and_then(|p2| draw::draw_with(&g, &p2, step, Charset::Unicode, lr_loops, &mut w2));
            match again {
                Ok(c2)
                    if c2.drawing.rows == c.drawing.rows
                        && c2.problems == c.problems
                        && c2.drawing.labels == c.drawing.labels =>
                {
                    t.deterministic += 1
                }
                _ => t.note("not deterministic", size, || format!("{set} {hash} {name}")),
            }
            let rows_ok = py_rows == rs_rows;
            let py_p = problems_of(s["m"].as_array().map(Vec::as_slice).unwrap_or(&[]));
            if lr_loops && py_p.dropped > 0 {
                // A left-to-right step the prototype failed for a self-loop.
                let fits78 = c.drawing.width <= 78;
                let only_drop = Problems { dropped: 0, ..py_p } == Problems::default();
                let kind = match (only_drop, c.problems.any()) {
                    (true, false) => "clean now",
                    (true, true) => "still has problems",
                    (false, _) => "had other problems too",
                };
                *t.lr_loop_steps
                    .entry(format!("{name}: {kind}{}", if fits78 { ", fits 78" } else { "" }))
                    .or_default() += 1;
                if only_drop && c.problems.any() {
                    t.note("lr self-loop step still has problems", size, || {
                        format!("{set} {hash} {name}: {:?}\n{}\n{}", c.problems, graph_text(&g), c.drawing.text())
                    });
                }
                if only_drop && !c.problems.any() {
                    t.note("lr self-loop step drawn", size, || format!("{set} {hash} {name}\n{}", c.drawing.text()));
                }
            }
            let counts_ok = py_p == c.problems;
            if rows_ok && counts_ok {
                t.steps_match += 1;
                t.steps_by_set.entry(set.clone()).or_default().1 += 1;
                continue;
            }
            let tag = if width_diff {
                " (label widths differ)"
            } else if seg_diff {
                " (graphemes differ)"
            } else {
                ""
            };
            differs |= tag.is_empty();
            let _ = writeln!(t.all, "{set} {hash} {name} rows_ok={rows_ok} counts_ok={counts_ok}{tag}");
            if !rows_ok {
                t.note(&format!("rows differ{tag}"), size, || {
                    format!(
                        "{set} {hash} {name}\n{}\n--- prototype\n{}\n--- rust\n{}\n",
                        graph_text(&g),
                        py_rows.join("\n"),
                        rs_rows.join("\n")
                    )
                });
            }
            if !counts_ok {
                let mut which = Vec::new();
                if py_p.text_over_line != c.problems.text_over_line {
                    which.push("text_over_line");
                }
                if py_p.false_pairs != c.problems.false_pairs {
                    which.push("false_pairs");
                }
                if py_p.bad_cells != c.problems.bad_cells {
                    which.push("bad_cells");
                }
                if py_p.hard_conflicts != c.problems.hard_conflicts {
                    which.push("hard_conflicts");
                }
                if py_p.dropped != c.problems.dropped {
                    which.push("dropped");
                }
                let kind = format!(
                    "counts differ: {}{tag}{}",
                    which.join(","),
                    if rows_ok { "" } else { ", rows differ too" }
                );
                t.note(&kind, size, || {
                    format!("{set} {hash} {name}: prototype {py_p:?}\n  rust {:?}\n{}", c.problems, graph_text(&g))
                });
            }
        }
        if differs {
            *t.group_differ.entry(group).or_default() += 1;
            if group == "none of these" {
                t.note("differs for another reason", size, || format!("{set} {hash}\n{}", graph_text(&g)));
            }
        }
        let ms = time_all(&g, lr_loops, repeat);
        t.times.push(ms);
        t.times_by_set.entry(set.clone()).or_default().push(ms);
        let elems: usize = prepared.order.iter().map(Vec::len).sum();
        let widest = prepared.order.iter().map(Vec::len).max().unwrap_or(0);
        t.slowest.push((
            ms,
            format!(
                "{set} {hash}: {} nodes, {} edges, {elems} elements, widest rank {widest}",
                g.nodes.len(),
                g.edges.len()
            ),
        ));
        t.slowest.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        t.slowest.truncate(5);
    }

    let mut report = String::new();
    let _ = writeln!(report, "diagrams {} {:?}", t.diagrams, t.by_set);
    let _ = writeln!(report, "label widths differ (python vs unicode-width) in {} diagrams", t.width_diff_diagrams);
    let _ = writeln!(report, "only grapheme segmentation differs in {} diagrams", t.seg_diff_diagrams);
    let _ = writeln!(report, "steps compared {}, identical rows and counts {}", t.steps, t.steps_match);
    for (set, (n, same)) in &t.steps_by_set {
        let _ = writeln!(report, "  {set}: {n} steps, {same} identical");
    }
    let _ = writeln!(report, "steps drawn identically twice (rows, problems, labels): {}", t.deterministic);
    let _ = writeln!(report, "python crashes: prepare {}, step {}", t.py_prep_crash, t.py_step_crash);
    for (k, n) in &t.crash_outcomes {
        let _ = writeln!(report, "  {n:5}  {k}");
    }
    let _ = writeln!(report, "rust too large: prepare {}, draw {}", t.too_large_prep, t.too_large_draw);
    let _ = writeln!(report, "mismatches by kind:");
    for (k, n) in &t.kinds {
        let _ = writeln!(report, "  {n:5}  {k}");
    }
    let _ = writeln!(report, "diagrams that differ, not counting label width or grapheme differences:");
    for (k, n) in &t.group_diagrams {
        let d = t.group_differ.get(k).copied().unwrap_or(0);
        let _ = writeln!(report, "  {d:5} of {n:5} with {k}");
    }
    let mut sets: Vec<(String, Vec<f64>)> = vec![("all".to_string(), t.times.clone())];
    sets.extend(t.times_by_set.iter().map(|(k, v)| (k.clone(), v.clone())));
    for (name, mut times) in sets {
        times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let _ = writeln!(
            report,
            "time per diagram, prepare + every ladder step + checks, fastest of {repeat} ({name}, {} diagrams): p50 {:.2} ms, p90 {:.2}, p99 {:.2}, p99.9 {:.2}, max {:.2}",
            times.len(),
            quantile(&times, 0.5),
            quantile(&times, 0.9),
            quantile(&times, 0.99),
            quantile(&times, 0.999),
            times.last().copied().unwrap_or(0.0)
        );
    }
    for (ms, what) in &t.slowest {
        let _ = writeln!(report, "  {ms:7.2} ms  {what}");
    }
    if lr_loops {
        let _ = writeln!(report, "left-to-right steps the prototype failed for a dropped self-loop:");
        for (k, n) in &t.lr_loop_steps {
            let _ = writeln!(report, "  {n:5}  {k}");
        }
    }
    print!("{report}");
    if let Some(dir) = out_dir {
        let mut ex = report.clone();
        for (k, v) in &t.examples {
            let _ = writeln!(ex, "\n==================== {k}");
            for (_, text) in v {
                let _ = writeln!(ex, "{text}\n");
            }
        }
        std::fs::write(format!("{dir}/mismatches.txt"), ex).expect("write mismatches");
        std::fs::write(format!("{dir}/mismatch_list.txt"), &t.all).expect("write mismatch list");
    }
}

/// Milliseconds to prepare `g` and draw and check it at every ladder step,
/// as the pager would: the fastest of `repeat` runs.
fn time_all(g: &FlowGraph, lr_loops: bool, repeat: usize) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..repeat.max(1) {
        let started = Instant::now();
        let mut work = Work::new(WORK_LIMIT);
        if let Ok(p) = prep::prepare(g, &mut work) {
            for &step in folio::diagram::ladder(g.dir) {
                let _ = draw::draw_with(g, &p, step, Charset::Unicode, lr_loops, &mut work);
            }
        }
        best = best.min(started.elapsed().as_secs_f64() * 1e3);
    }
    best
}

/// Time painting, the checks and rendering separately at every step (the
/// fastest of 5 runs each).
fn profile_phases(g: &FlowGraph, p: &prep::Prepared, hash: &str) {
    let (mut paint, mut checks, mut render) = (f64::MAX, f64::MAX, f64::MAX);
    for _ in 0..5 {
        let (mut a, mut b, mut c) = (0.0, 0.0, 0.0);
        for &step in folio::diagram::ladder(g.dir) {
            let mut work = Work::new(WORK_LIMIT);
            let t0 = Instant::now();
            let prof = folio::diagram::flow::Profile::new(step, g.dir.base());
            let Ok(painted) = draw::paint(g, p, &prof, false, &mut work) else { return };
            let t1 = Instant::now();
            if a == 0.0 && paint == f64::MAX {
                eprintln!("  {step:?}: {} cells", painted.canvas.len());
            }
            let edges: Vec<Option<(usize, usize)>> = g.edges.iter().map(|e| Some((e.from, e.to))).collect();
            let _ = check::cell_check(&painted.canvas, &mut work);
            let tc = Instant::now();
            let _ = check::flow_check(&painted.canvas, &edges, &[], &painted.joins, &mut work);
            let t2 = Instant::now();
            if paint == f64::MAX && a == 0.0 {
                eprintln!("  cell_check {:?} flow_check {:?}", tc - t1, t2 - tc);
            }
            let _ = painted.canvas.render();
            let t3 = Instant::now();
            a += (t1 - t0).as_secs_f64();
            b += (t2 - t1).as_secs_f64();
            c += (t3 - t2).as_secs_f64();
        }
        paint = paint.min(a);
        checks = checks.min(b);
        render = render.min(c);
    }
    eprintln!("{hash}: paint {:.2} ms, checks {:.2} ms, render {:.2} ms", paint * 1e3, checks * 1e3, render * 1e3);
}

/// The graph as short text, for the examples.
fn graph_text(g: &FlowGraph) -> String {
    let mut s = format!("{:?}:", g.dir);
    for (i, n) in g.nodes.iter().enumerate() {
        let _ = write!(s, " {i}={:?}[{:?}]{}", n.shape, n.label, if n.dead { "(dead)" } else { "" });
    }
    for e in &g.edges {
        let _ = write!(
            s,
            "\n  {}-{:?}/{:?}/{:?}->{} len {} {:?}",
            e.from, e.stroke, e.head_from, e.head_to, e.to, e.min_len, e.label
        );
    }
    s
}
