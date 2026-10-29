//! Compare folio's Mermaid parsers and outlines with the Python prototype's.
//!
//! usage: cargo run --release --example mermaid_parse_diff -- PROTO.jsonl
//!
//! `PROTO.jsonl` comes from `docs/mermaid-prototype/proto2/m1_dump.py`: one
//! line per corpus block with the block's source and what the prototype
//! made of it. The corpus is third-party content, so it and the file stay
//! outside the repository.
//!
//! The report counts mismatches by category, with a few examples of each.
//! Categories that start with "deliberate:" are changes the plan asks for.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use folio::diagram::flow::{self, Dir, FlowGraph, Head, Shape, Stroke};
use folio::diagram::seq::{self, Event, SeqDiagram, SeqHead};
use folio::diagram::{Drawing, NotDrawn, outline, pre};
use serde_json::{Value, json};

const FLOW: &[&str] = &["flowchart", "graph", "flowchart-v2", "flowchart-elk"];
const WIDE: usize = 1_000_000;
const EXAMPLES: usize = 4;

#[derive(Default)]
struct Tally {
    counts: BTreeMap<String, usize>,
    examples: BTreeMap<String, Vec<String>>,
}

impl Tally {
    fn add(&mut self, cat: impl Into<String>, example: impl FnOnce() -> String) {
        let cat = cat.into();
        *self.counts.entry(cat.clone()).or_default() += 1;
        let ex = self.examples.entry(cat).or_default();
        if ex.len() < EXAMPLES {
            ex.push(example());
        }
    }
}

/// What folio made of one block.
enum Rust {
    /// Not read: the reason.
    Failed(String),
    Other,
    Flow(Result<FlowGraph, String>),
    Seq(Result<SeqDiagram, String>),
}

fn reason(e: NotDrawn) -> String {
    match e {
        NotDrawn::Unsupported(why) => why,
        NotDrawn::TooLarge => "too large to draw here".into(),
    }
}

fn run(src: &str) -> (Option<String>, Rust) {
    let (title, body) = match pre::preamble(src) {
        Ok(p) => p,
        Err(e) => return (None, Rust::Failed(reason(e))),
    };
    let st = pre::statements(&body);
    let Some(first) = st.first() else {
        return (title, Rust::Failed("empty diagram".into()));
    };
    let kw = first.split_whitespace().next().unwrap_or("");
    let r = if FLOW.contains(&kw) {
        Rust::Flow(flow::parse::parse(first, &st[1..]).map_err(reason))
    } else if kw == "sequenceDiagram" {
        Rust::Seq(seq::parse::parse(first, &st[1..]).map_err(reason))
    } else {
        Rust::Other
    };
    (title, r)
}

/// A reason without the user text it quotes; the prototype's wording for
/// an empty diagram is folded into the plan's.
fn reason_kind(r: &str) -> String {
    match r {
        "no nodes" | "no participants" | "empty diagram" | "nothing to draw" => "empty or nothing to draw".into(),
        r => r.split('"').next().unwrap_or("").trim().to_string(),
    }
}

fn dir_name(d: Dir) -> &'static str {
    match d {
        Dir::TD => "TD",
        Dir::LR => "LR",
        Dir::BT => "BT",
        Dir::RL => "RL",
    }
}

fn shape_name(s: Shape) -> &'static str {
    match s {
        Shape::Rect => "rect",
        Shape::Round => "round",
        Shape::Stadium => "stadium",
        Shape::Subroutine => "subroutine",
        Shape::Cylinder => "cylinder",
        Shape::Circle => "circle",
        Shape::DCircle => "dcircle",
        Shape::Decision => "decision",
        Shape::Hexagon => "hexagon",
        Shape::Slant => "slant",
        Shape::Asym => "asym",
        Shape::Bare => "bare",
    }
}

fn stroke_name(s: Stroke) -> &'static str {
    match s {
        Stroke::Solid => "solid",
        Stroke::Dotted => "dotted",
        Stroke::Thick => "thick",
        Stroke::Invisible => "invisible",
    }
}

fn head_name(h: Head) -> &'static str {
    match h {
        Head::None => "none",
        Head::Arrow => "arrow",
        Head::Circle => "circle",
        Head::Cross => "cross",
    }
}

/// The graph in the prototype's form.
fn flow_json(g: &FlowGraph) -> Value {
    let cid = |c: Option<usize>| c.map(|c| g.clusters[c].id.clone());
    let nodes: Vec<Value> = g
        .nodes
        .iter()
        .map(|n| {
            json!({"id": n.id, "label": n.label, "shape": shape_name(n.shape),
                   "slant": n.slant.map(|(a, b)| vec![a.to_string(), b.to_string()]),
                   "explicit": n.explicit, "dead": n.dead, "cluster": cid(n.cluster)})
        })
        .collect();
    let edges: Vec<Value> = g
        .edges
        .iter()
        .map(|e| {
            json!({"a": e.from, "b": e.to, "stroke": stroke_name(e.stroke), "ha": head_name(e.head_from),
                   "hb": head_name(e.head_to), "label": e.label, "minlen": e.min_len})
        })
        .collect();
    let clusters: Vec<Value> = g
        .clusters
        .iter()
        .map(|c| json!({"id": c.id, "title": c.title, "parent": cid(c.parent), "members": c.members}))
        .collect();
    json!({"dir": dir_name(g.dir), "nodes": nodes, "edges": edges, "clusters": clusters,
           "simplified": g.simplified.iter().collect::<Vec<_>>()})
}

fn arrow_name(stroke: Stroke, head: SeqHead, both: bool) -> String {
    let dash = if stroke == Stroke::Dotted { "--" } else { "-" };
    if both {
        return format!("<<{dash}>>");
    }
    let end = match head {
        SeqHead::None => ">",
        SeqHead::Arrow => ">>",
        SeqHead::Cross => "x",
        SeqHead::Async => ")",
    };
    format!("{dash}{end}")
}

/// The sequence diagram in the prototype's form.
fn seq_json(sd: &SeqDiagram) -> Value {
    let parts: Vec<Value> =
        sd.participants.iter().map(|p| json!({"id": p.id, "label": p.label, "kind": p.kind.name()})).collect();
    let events: Vec<Value> = sd
        .events
        .iter()
        .map(|ev| match ev {
            Event::Message { from, to, stroke, head, both, label, act } => {
                let act = match act {
                    Some(true) => "+",
                    Some(false) => "-",
                    None => "",
                };
                json!(["msg", from, to, arrow_name(*stroke, *head, *both), act, label])
            }
            Event::Note { at, who, label } => json!(["note", at.name(), who, label]),
            Event::Open { kind, label } => json!(["open", kind.name(), label]),
            Event::Divider { kind, label } => json!(["divider", kind.name(), label]),
            Event::Close => json!(["close"]),
            Event::Activate(k) => json!(["activate", k]),
            Event::Deactivate(k) => json!(["deactivate", k]),
            Event::Destroy(k) => json!(["destroy", k]),
        })
        .collect();
    json!({"parts": parts, "events": events, "autonumber": sd.autonumber.map(|(a, b)| vec![a, b]),
           "simplified": sd.simplified.iter().collect::<Vec<_>>()})
}

fn snippet(src: &str) -> String {
    let s: String = src.chars().take(300).collect();
    s.replace('\n', "⏎")
}

fn show(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() > 200 { s.chars().take(200).collect::<String>() + "…" } else { s }
}

/// Compare two values field by field; returns the first differing path.
fn first_diff(path: &str, py: &Value, rs: &Value) -> Option<(String, Value, Value)> {
    match (py, rs) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, va) in a {
                let vb = b.get(k).unwrap_or(&Value::Null);
                if let Some(d) = first_diff(&format!("{path}.{k}"), va, vb) {
                    return Some(d);
                }
            }
            None
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            a.iter().zip(b).enumerate().find_map(|(i, (x, y))| first_diff(&format!("{path}[{i}]"), x, y))
        }
        (Value::Array(_), Value::Array(_)) => Some((format!("{path}.len"), py.clone(), rs.clone())),
        _ if py == rs => None,
        _ => Some((path.to_string(), py.clone(), rs.clone())),
    }
}

/// Field paths without indexes, for categories: `nodes[3].label` → `nodes.label`.
fn path_kind(p: &str) -> String {
    let mut out = String::new();
    let mut skip = false;
    for c in p.chars() {
        match c {
            '[' => skip = true,
            ']' => skip = false,
            c if !skip => out.push(c),
            _ => {}
        }
    }
    out
}

fn row_text(d: &Drawing, r: usize) -> String {
    d.rows[r].iter().map(|(_, s)| s.as_str()).collect()
}

fn rows(d: &Drawing) -> Vec<String> {
    (0..d.rows.len()).map(|r| row_text(d, r)).collect()
}

fn py_rows(rec: &Value, w: &str) -> Vec<String> {
    rec["outline"][w].as_array().map(|a| a.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default()
}

/// Checks every outline must pass at width `w`: rows fit, no trailing
/// spaces, the width is the widest row, and every row is in some label.
fn check_fit(d: &Drawing, w: usize) -> Option<String> {
    let mut widest = 0;
    for r in 0..d.rows.len() {
        let t = row_text(d, r);
        let tw = pre::width(&t);
        widest = widest.max(tw);
        if tw > w {
            return Some(format!("row {r} is {tw} columns at width {w}: {t:?}"));
        }
        if t.ends_with(' ') {
            return Some(format!("row {r} ends with a space at width {w}: {t:?}"));
        }
    }
    if widest != d.width {
        return Some(format!("width {} but widest row {widest}", d.width));
    }
    let pieces: usize = d.labels.iter().map(|l| l.pieces.len()).sum();
    if pieces != d.rows.len() {
        return Some(format!("{pieces} label pieces for {} rows", d.rows.len()));
    }
    None
}

/// Every label's text, without spaces, at width `w` equals the unwrapped
/// line's: wrapping lost or added nothing.
fn check_text(d: &Drawing, wide: &Drawing) -> bool {
    let squash = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    d.labels.len() == wide.labels.len()
        && d.labels.iter().zip(&wide.labels).all(|(a, b)| {
            let ja: String = a.pieces.iter().map(|p| squash(&p.2)).collect();
            let jb: String = b.pieces.iter().map(|p| squash(&p.2)).collect();
            ja == jb
        })
}

fn compare_flow_outline(rec: &Value, g: &FlowGraph, t: &mut Tally, src: &str) {
    let wide = outline::flow_outline(g, WIDE);
    let mut rs = rows(&wide);
    let py = py_rows(rec, &WIDE.to_string());
    let mut deliberate = false;
    if !g.clusters.is_empty() {
        rs.drain(..g.clusters.len().min(rs.len()));
        t.add("deliberate: outline lists subgraphs first", || snippet(src));
        deliberate = true;
    }
    let visible: Vec<_> = g.edges.iter().filter(|e| e.stroke != Stroke::Invisible).collect();
    let n = visible.len().min(rs.len()).min(py.len());
    let mut bad = false;
    for (i, e) in visible.iter().enumerate().take(n) {
        if py[i] == rs[i] {
            continue;
        }
        let glyph = match e.head_from {
            Head::Circle => Some("o"),
            Head::Cross => Some("✕"),
            _ => None,
        };
        let line = match e.stroke {
            Stroke::Dotted => "┄",
            Stroke::Thick => "━",
            _ => "─",
        };
        if let Some(gl) = glyph
            && rs[i].replacen(&format!(" {gl}{line}"), &format!(" {line}"), 1) == py[i]
        {
            t.add("deliberate: outline shows o and ✕ heads at the start", || format!("py {:?} rs {:?}", py[i], rs[i]));
            deliberate = true;
            continue;
        }
        t.add("MISMATCH outline edge line", || format!("py {:?}\n      rs {:?}\n      src {}", py[i], rs[i], snippet(src)));
        bad = true;
    }
    if !bad {
        let (py_tail, rs_tail) = (&py[n.min(py.len())..], &rs[n.min(rs.len())..]);
        if py_tail != rs_tail {
            // Nodes linked only by invisible links are listed by folio.
            let mut linked_visible = vec![false; g.nodes.len()];
            let mut linked_any = vec![false; g.nodes.len()];
            for e in &g.edges {
                linked_any[e.from] = true;
                linked_any[e.to] = true;
                if e.stroke != Stroke::Invisible {
                    linked_visible[e.from] = true;
                    linked_visible[e.to] = true;
                }
            }
            let expected: Vec<String> = g
                .nodes
                .iter()
                .enumerate()
                .filter(|(k, n)| !linked_any[*k] && !n.dead)
                .map(|(k, n)| if n.label.is_empty() { g.nodes[k].id.clone() } else { n.label.replace('\n', " ") })
                .collect();
            let only_invisible = g.nodes.iter().enumerate().any(|(k, n)| linked_any[k] && !linked_visible[k] && !n.dead);
            if py_tail == expected.as_slice() && only_invisible {
                t.add("deliberate: outline lists nodes linked only by ~~~", || snippet(src));
                deliberate = true;
            } else {
                t.add("MISMATCH outline node lines", || {
                    format!("py {:?}\n      rs {:?}\n      src {}", py_tail, rs_tail, snippet(src))
                });
                bad = true;
            }
        }
    }
    for w in [40usize, 78] {
        let d = outline::flow_outline(g, w);
        if let Some(why) = check_fit(&d, w) {
            t.add("BUG outline does not fit", || format!("{why}\n      src {}", snippet(src)));
        }
        if !check_text(&d, &wide) {
            t.add("BUG outline wrapping lost text", || snippet(src));
        }
        let pyw = py_rows(rec, &w.to_string());
        if rows(&d) != pyw && !bad {
            if deliberate {
                t.add(format!("rows at {w} differ: follows the deliberate changes"), String::new);
            } else if proto_flow_rows(&py, w) == pyw {
                t.add(format!("deliberate: rows at {w}: broken words are not re-joined with a space"), || {
                    format!("py {:?}\n      rs {:?}", pyw, rows(&d))
                });
            } else {
                t.add(format!("rows at {w}: character widths differ (unicodedata vs unicode-width)"), || {
                    format!("py {:?}\n      rs {:?}", pyw, rows(&d))
                });
            }
        }
    }
    for w in [1usize, 2, 3, 4, 5, 7, 10, 15, 20, 30] {
        if let Some(why) = check_fit(&outline::flow_outline(g, w), w) {
            t.add("BUG outline does not fit (narrow)", || format!("{why}\n      src {}", snippet(src)));
        }
    }
}

/// The prototype's outline wrapping, with folio's widths: the first row at
/// `w`, the rest joined with spaces and wrapped at `w - 2`.
fn proto_flow_rows(lines: &[String], w: usize) -> Vec<String> {
    let mut out = Vec::new();
    for l in lines {
        let ws = pre::wrap(l, w);
        out.push(ws[0].clone());
        if ws.len() > 1 {
            out.extend(pre::wrap(&ws[1..].join(" "), w.saturating_sub(2)).into_iter().map(|c| format!("  {c}")));
        }
    }
    out
}

/// The prototype's transcript wrapping, with folio's widths: each line at
/// the width left after its indent, continued 3 columns further in.
fn proto_seq_rows(lines: &[String], w: usize) -> Vec<String> {
    let mut out = Vec::new();
    for l in lines {
        let text = l.trim_start();
        let ind = &l[..l.len() - text.len()];
        let ws = pre::wrap(text.trim(), w.saturating_sub(ind.len()));
        out.push(format!("{ind}{}", ws[0]));
        out.extend(ws[1..].iter().map(|c| format!("{ind}   {c}")));
    }
    out
}

fn compare_seq_transcript(rec: &Value, sd: &SeqDiagram, t: &mut Tally, src: &str) {
    let wide = outline::seq_transcript(sd, WIDE);
    let rs = rows(&wide);
    let py = py_rows(rec, &WIDE.to_string());
    let mut deliberate = false;
    let mut bad = false;
    if rs != py {
        // The prototype draws every arrow as ─▸ or ┄▸ (and <<-->> as ─▸).
        let as_proto: Vec<String> = rs
            .iter()
            .map(|r| r.replace("◂┄▸", "─▸").replace("◂─▸", "─▸").replace("─✕", "─▸").replace("┄✕", "┄▸"))
            .collect();
        let newline = src.contains("<br") || src.contains("#10;");
        if as_proto == py {
            t.add("deliberate: transcript shows ◂ and ✕ heads", || format!("py {:?}\n      rs {:?}", py, rs));
            deliberate = true;
        } else if newline && py.len() > rs.len() {
            t.add("deliberate: transcript keeps a label with a line break on one line", || {
                format!("py {:?}\n      rs {:?}", py, rs)
            });
            deliberate = true;
        } else {
            t.add("MISMATCH transcript lines", || format!("py {:?}\n      rs {:?}\n      src {}", py, rs, snippet(src)));
            bad = true;
        }
    }
    for w in [40usize, 78] {
        let d = outline::seq_transcript(sd, w);
        if let Some(why) = check_fit(&d, w) {
            t.add("BUG transcript does not fit", || format!("{why}\n      src {}", snippet(src)));
        }
        if !check_text(&d, &wide) {
            t.add("BUG transcript wrapping lost text", || snippet(src));
        }
        let pyw = py_rows(rec, &w.to_string());
        if rows(&d) != pyw && !bad {
            if deliberate {
                t.add(format!("rows at {w} differ: follows the deliberate changes"), String::new);
            } else if proto_seq_rows(&py, w) == pyw {
                t.add(format!("deliberate: transcript rows at {w} use a 2-column hanging indent"), || {
                    format!("py {:?}\n      rs {:?}", pyw, rows(&d))
                });
            } else {
                t.add(format!("transcript rows at {w}: character widths differ (unicodedata vs unicode-width)"), || {
                    format!("py {:?}\n      rs {:?}", pyw, rows(&d))
                });
            }
        }
    }
    for w in [1usize, 2, 3, 4, 5, 7, 10, 15, 20, 30] {
        if let Some(why) = check_fit(&outline::seq_transcript(sd, w), w) {
            t.add("BUG transcript does not fit (narrow)", || format!("{why}\n      src {}", snippet(src)));
        }
    }
}

/// Parse counts for one sample and diagram type.
#[derive(Default)]
struct Rate {
    blocks: usize,
    py_parsed: usize,
    py_crashed: usize,
    rs_parsed: usize,
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: mermaid_parse_diff PROTO.jsonl");
    let text = std::fs::read_to_string(&path).expect("read the prototype's results");
    let mut t = Tally::default();
    let mut rates: BTreeMap<(String, &str), Rate> = BTreeMap::new();
    let mut panics = 0;
    let mut records = 0;
    for line in text.lines() {
        let rec: Value = serde_json::from_str(line).expect("valid JSON line");
        records += 1;
        let src = rec["src"].as_str().unwrap_or("").to_string();
        let set = rec["set"].as_str().unwrap_or("?").to_string();
        let py_kind = rec["kind"].as_str().unwrap_or("?");
        let Ok((title, rs)) = catch_unwind(AssertUnwindSafe(|| run(&src))) else {
            panics += 1;
            t.add("BUG panic", || snippet(&src));
            continue;
        };
        let py_title = rec["title"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
        if py_kind != "none" && py_title != title {
            let cat = if py_title.is_none() && src.contains("\ncaption:") {
                "deliberate: front-matter caption used as the title"
            } else {
                "MISMATCH title"
            };
            t.add(cat, || format!("py {py_title:?} rs {title:?} src {}", snippet(&src)));
        }
        let rs_kind = match &rs {
            Rust::Failed(_) => "none",
            Rust::Other => "other",
            Rust::Flow(_) => "flow",
            Rust::Seq(_) => "seq",
        };
        if py_kind != rs_kind {
            t.add(format!("MISMATCH kind: py {py_kind} rs {rs_kind}"), || snippet(&src));
            continue;
        }
        let py_why = rec["why"].as_str();
        let py_crash = rec["crash"].as_str();
        let ty = match py_kind {
            "flow" => "flowchart",
            "seq" => "sequence",
            _ => {
                if let (Rust::Failed(rw), Some(pw)) = (&rs, py_why)
                    && reason_kind(rw) != reason_kind(pw)
                {
                    t.add(format!("reason differs: py {pw:?} rs {rw:?}"), || snippet(&src));
                }
                continue;
            }
        };
        let rate = rates.entry((set.clone(), ty)).or_default();
        rate.blocks += 1;
        let py_ok = rec.get("g").is_some() || rec.get("sd").is_some();
        rate.py_parsed += usize::from(py_ok);
        rate.py_crashed += usize::from(py_crash.is_some());
        let rs_res: Result<Value, String> = match &rs {
            Rust::Flow(r) => r.as_ref().map(flow_json).map_err(Clone::clone),
            Rust::Seq(r) => r.as_ref().map(seq_json).map_err(Clone::clone),
            _ => unreachable!(),
        };
        rate.rs_parsed += usize::from(rs_res.is_ok());
        if let Err(why) = &rs_res
            && why.chars().any(char::is_control)
        {
            t.add("BUG reason has a control character", || why.clone());
        }
        match (&rs_res, py_ok) {
            (Err(rw), _) if py_crash.is_some() => {
                t.add(format!("prototype crashed ({}); folio: {}", py_crash.unwrap_or(""), reason_kind(rw)), || {
                    snippet(&src)
                });
            }
            (Ok(_), _) if py_crash.is_some() => {
                t.add(format!("prototype crashed ({}); folio parses", py_crash.unwrap_or("")), || snippet(&src));
            }
            (Err(rw), false) => {
                let pw = py_why.unwrap_or("");
                let (a, b) = (reason_kind(pw), reason_kind(rw));
                if a != b {
                    t.add(format!("{ty}: reason differs: py {a:?} rs {b:?}"), || {
                        format!("py {pw:?} rs {rw:?}\n      src {}", snippet(&src))
                    });
                } else if pw != rw {
                    t.add(format!("deliberate: {ty}: same reason, plan wording or quote cut to 30 columns ({a})"), || {
                        format!("py {pw:?} rs {rw:?}")
                    });
                }
            }
            (Ok(_), false) if ty == "sequence" && src.contains("accDescr") && src.contains('{') => {
                t.add("deliberate: sequence: multi-line accDescr { … } is skipped", || {
                    format!("{}\n      src {}", py_why.unwrap_or(""), snippet(&src))
                });
            }
            (Err(rw), true) => {
                t.add(format!("MISMATCH {ty}: folio rejects ({})", reason_kind(rw)), || {
                    format!("{rw}\n      src {}", snippet(&src))
                });
            }
            (Ok(_), false) => {
                let pw = py_why.unwrap_or("");
                t.add(format!("MISMATCH {ty}: prototype rejects ({})", reason_kind(pw)), || {
                    format!("{pw}\n      src {}", snippet(&src))
                });
            }
            (Ok(rv), true) => {
                let pv = if ty == "flowchart" { &rec["g"] } else { &rec["sd"] };
                let mut pv = pv.clone();
                let mut rv = rv.clone();
                // Side glyphs only matter for slanted shapes.
                if ty == "flowchart" {
                    for v in [&mut pv, &mut rv] {
                        for n in v["nodes"].as_array_mut().into_iter().flatten() {
                            if !matches!(n["shape"].as_str(), Some("slant" | "asym")) {
                                n["slant"] = Value::Null;
                            }
                        }
                    }
                }
                if let Some((p, a, b)) = first_diff("", &pv, &rv) {
                    // The prototype can move an edge onto a subgraph id that
                    // was itself moved, so the edge ends at a node that is not drawn.
                    let dead_end = (p.ends_with(".a") || p.ends_with(".b"))
                        && a.as_u64().is_some_and(|k| pv["nodes"][k as usize]["dead"] == Value::Bool(true));
                    let cat = if dead_end {
                        "deliberate: flowchart: an edge to a subgraph never ends at a hidden node".to_string()
                    } else {
                        format!("MISMATCH {ty} IR: {}", path_kind(&p))
                    };
                    t.add(cat, || format!("{p}: py {} rs {}\n      src {}", show(&a), show(&b), snippet(&src)));
                    continue;
                }
                let ok = catch_unwind(AssertUnwindSafe(|| match &rs {
                    Rust::Flow(Ok(g)) => compare_flow_outline(&rec, g, &mut t, &src),
                    Rust::Seq(Ok(sd)) => compare_seq_transcript(&rec, sd, &mut t, &src),
                    _ => {}
                }));
                if ok.is_err() {
                    panics += 1;
                    t.add("BUG panic in outline", || snippet(&src));
                }
            }
        }
    }
    println!("records {records}, folio panics {panics}\n");
    println!("parse rates (folio / prototype, of blocks of that type; prototype crashes count as failures):");
    for ((set, ty), r) in &rates {
        let pct = |n: usize| 100.0 * n as f64 / r.blocks.max(1) as f64;
        println!(
            "  {set:5} {ty:10} blocks {:6}  folio {:6} ({:5.1}%)  prototype {:6} ({:5.1}%)  prototype crashes {}",
            r.blocks,
            r.rs_parsed,
            pct(r.rs_parsed),
            r.py_parsed,
            pct(r.py_parsed),
            r.py_crashed
        );
    }
    println!("\ncategories:");
    for (cat, n) in &t.counts {
        println!("{n:7}  {cat}");
    }
    println!("\nexamples:");
    for (cat, ex) in &t.examples {
        println!("== {cat}");
        for e in ex.iter().filter(|e| !e.is_empty()) {
            println!("    - {e}");
        }
    }
}
