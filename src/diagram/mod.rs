//! Mermaid diagrams drawn as text.
//!
//! [`render`] turns a diagram's source into one of three outcomes:
//!
//! - **Drawn:** a drawing that fits the width and passed every check.
//! - **Outline:** a flowchart outline or sequence transcript, when no
//!   drawing fits or the diagram is too large. It always fits.
//! - **Source:** the diagram can't be read; the reason is shown under the
//!   source.
//!
//! The design is in `docs/mermaid-plan.md`; the Python prototype this code
//! was ported from is in `docs/mermaid-prototype/proto2/`.

pub mod canvas;
pub mod check;
pub mod flow;
pub mod outline;
pub mod pre;
pub mod seq;

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use flow::{Dir, FlowGraph};
use seq::SeqDiagram;

/// Characters a drawing may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Charset {
    Unicode,
    /// ASCII only (M3).
    Ascii,
}

/// What `--diagrams` asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Draw,
    /// Today's code box for every Mermaid block.
    Source,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Flowchart,
    Sequence,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Flowchart => "flowchart",
            Kind::Sequence => "sequence",
        }
    }
}

/// What a drawn character is part of; layout maps roles to theme styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Role {
    Border,
    NodeText,
    Edge,
    EdgeLabel,
    Arrow,
    Frame,
    FrameTitle,
    Note,
}

/// One step of the ladder (plan section 7.1). Each step is tried in turn
/// until a drawing fits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Step {
    Normal,
    Compact,
    /// Left to right, labels wrapped at 16 columns.
    LrLabels16,
    /// Left to right, labels wrapped at 8 columns.
    LrLabels8,
    /// A left-to-right diagram drawn top-down.
    Turned,
    TurnedCompact,
    Labels16,
    Labels8,
}

impl Step {
    /// Whether this step draws a left-to-right diagram top-down.
    pub fn turned(self) -> bool {
        matches!(self, Step::Turned | Step::TurnedCompact | Step::Labels16 | Step::Labels8)
    }
}

/// A label as drawn, for search: one piece per row it occupies, as
/// (row, column, text). A node label's lines are one label.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabelRef {
    pub pieces: Vec<(usize, usize, String)>,
}

/// A drawing, outline or transcript, ready for layout to frame.
#[derive(Clone, Debug)]
pub struct Drawing {
    pub kind: Kind,
    /// The diagram's title (front matter, or its own `title` statement),
    /// shown in the top rule.
    pub title: Option<String>,
    /// The ladder step it was drawn at; `None` for outlines and transcripts.
    pub step: Option<Step>,
    /// Each row is a list of runs. Rows carry no trailing spaces.
    pub rows: Vec<Vec<(Role, String)>>,
    /// The widest row, in columns.
    pub width: usize,
    /// Every label as drawn, for search and reading time.
    pub labels: Vec<LabelRef>,
    /// Shown in the bottom rule, for example "drawn top-down to fit".
    pub notes: Vec<String>,
}

impl Drawing {
    /// The rows as plain text, for copying.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for row in &self.rows {
            for (_, run) in row {
                s.push_str(run);
            }
            s.push('\n');
        }
        s
    }
}

/// What the checks found in one drawing (plan section 6.2 M). A drawing is
/// shown only when every count is zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Problems {
    /// Label text painted over a line.
    pub text_over_line: usize,
    /// Pairs of nodes that the lines seem to connect but no edge joins.
    pub false_pairs: usize,
    /// Cells where two buses overlap other than in a clean crossing.
    pub bad_cells: usize,
    /// Router constraints that could not all be met.
    pub hard_conflicts: usize,
    /// Edges or self-loops that could not be drawn at all.
    pub dropped: usize,
}

impl Problems {
    pub fn any(&self) -> bool {
        *self != Problems::default()
    }
}

/// A drawing and what the checks found in it.
#[derive(Clone, Debug)]
pub struct Checked {
    pub drawing: Drawing,
    pub problems: Problems,
}

pub enum Outcome {
    Drawn(Arc<Drawing>),
    Outline(Arc<Drawing>),
    /// Not drawn; the reason, shown as "Not drawn: <reason>".
    Source(String),
}

pub struct Request {
    pub width: usize,
    pub charset: Charset,
}

/// Why a diagram was not drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotDrawn {
    /// The source can't be read. The text completes "Not drawn: …".
    Unsupported(String),
    /// Over a size cap or the work limit: the outline is shown instead.
    TooLarge,
}

impl NotDrawn {
    pub fn unsupported(why: impl Into<String>) -> NotDrawn {
        NotDrawn::Unsupported(why.into())
    }
}

/// Inner-loop steps one diagram may take, across all its ladder steps.
/// The engine does about 15,000 to 29,000 steps per millisecond in a
/// release build, so a diagram that hits the limit has spent roughly
/// 17 to 33 ms. The corpus's p99.9 diagram needs about 400,000 (plan
/// section 6.6); about 0.1% of diagrams go over and get the outline.
pub const WORK_LIMIT: u64 = 500_000;

/// Counts inner-loop steps so one diagram can't stall the pager. It never
/// looks at the clock, so the output is the same on every machine.
#[derive(Clone, Debug)]
pub struct Work {
    left: u64,
}

impl Work {
    pub fn new(limit: u64) -> Work {
        Work { left: limit }
    }

    /// Spend `n` steps; fails with [`NotDrawn::TooLarge`] once the limit is
    /// used up.
    pub fn spend(&mut self, n: u64) -> Result<(), NotDrawn> {
        if n > self.left {
            self.left = 0;
            return Err(NotDrawn::TooLarge);
        }
        self.left -= n;
        Ok(())
    }
}

/// Size caps (plan section 6.6). Over a cap, the outline is shown.
pub const MAX_NODES: usize = 150;
pub const MAX_EDGES: usize = 300;
pub const MAX_SOURCE: usize = 64 * 1024;
pub const MAX_PARTICIPANTS: usize = 40;
pub const MAX_EVENTS: usize = 500;

/// Sources up to this size are still parsed, so one over [`MAX_SOURCE`]
/// gets its outline. Larger ones aren't read at all and show their source.
pub const MAX_PARSE: usize = 4 * MAX_SOURCE;

const TOO_LARGE: &str = "too large to draw here";

/// Entries the cache keeps before evicting the least recently used.
const CACHE_CAP: usize = 1024;

/// Parsed diagrams and their drawings, kept across layouts like the
/// syntax highlighter's cache. Lookups go through hash maps that are never
/// iterated to produce output, so output never depends on their order.
pub struct Diagrams {
    pub mode: Mode,
    /// Diagrams the reader switched to source, keyed by (hash of the
    /// source, occurrence among the document's diagrams with that hash).
    /// They belong to the document being shown; the pager swaps them when
    /// another document opens.
    pub as_source: HashSet<(u64, u32)>,
    entries: HashMap<u64, Entry>,
    /// Number of the current layout; entries remember the last layout
    /// that used them, and those are never evicted.
    layout_no: u64,
}

struct Entry {
    source: String,
    used: u64,
    title: Option<String>,
    parsed: Parsed,
    /// Shared by every ladder step of this diagram.
    work: Work,
    drawings: Vec<(Step, Charset, StepResult)>,
    outlines: Vec<(usize, Arc<Drawing>)>,
}

/// A drawing at one ladder step and what the checks found, or why there is none.
type StepResult = Result<(Arc<Drawing>, Problems), NotDrawn>;

enum Parsed {
    Failed(String),
    Flow { graph: FlowGraph, prep: Option<Result<flow::prep::Prepared, NotDrawn>> },
    Seq(SeqDiagram),
}

impl Default for Diagrams {
    fn default() -> Diagrams {
        Diagrams::new(Mode::Draw)
    }
}

impl Diagrams {
    pub fn new(mode: Mode) -> Diagrams {
        Diagrams { mode, as_source: HashSet::new(), entries: HashMap::new(), layout_no: 0 }
    }

    /// Called at the start of each layout, so the cache knows which entries
    /// are in use.
    pub fn begin_layout(&mut self) {
        self.layout_no += 1;
    }

    fn entry(&mut self, source: &str) -> &mut Entry {
        let key = source_hash(source);
        let fresh = self.entries.get(&key).is_none_or(|e| e.source != source);
        if fresh {
            if self.entries.len() >= CACHE_CAP {
                self.evict();
            }
            let (title, parsed) = parse(source);
            let entry = Entry {
                source: source.to_string(),
                used: self.layout_no,
                title,
                parsed,
                work: Work::new(WORK_LIMIT),
                drawings: Vec::new(),
                outlines: Vec::new(),
            };
            self.entries.insert(key, entry);
        }
        let entry = self.entries.get_mut(&key).expect("just inserted");
        entry.used = self.layout_no;
        entry
    }

    /// Drop the least recently used entries, never one the current layout
    /// has used.
    fn evict(&mut self) {
        let mut old: Vec<(u64, u64)> =
            self.entries.iter().filter(|(_, e)| e.used < self.layout_no).map(|(&k, e)| (e.used, k)).collect();
        old.sort_unstable();
        for (_, k) in old.into_iter().take(CACHE_CAP / 4) {
            self.entries.remove(&k);
        }
    }
}

/// The hash a diagram's source is cached and toggled under.
pub fn source_hash(source: &str) -> u64 {
    let mut h = DefaultHasher::new();
    source.hash(&mut h);
    h.finish()
}

/// Parse a diagram: its front-matter title and its graph or sequence.
fn parse(source: &str) -> (Option<String>, Parsed) {
    if source.len() > MAX_PARSE {
        return (None, Parsed::Failed(TOO_LARGE.into()));
    }
    let (title, body) = match pre::preamble(source) {
        Ok(p) => p,
        Err(e) => return (None, Parsed::Failed(reason(e))),
    };
    let statements = pre::statements(&body);
    let Some(first) = statements.first() else {
        return (title, Parsed::Failed("empty diagram".into()));
    };
    let keyword = first.split_whitespace().next().unwrap_or("");
    let parsed = match keyword {
        "flowchart" | "graph" | "flowchart-v2" | "flowchart-elk" => {
            match flow::parse::parse(first, &statements[1..]) {
                Ok(graph) => Parsed::Flow { graph, prep: None },
                Err(e) => Parsed::Failed(reason(e)),
            }
        }
        "sequenceDiagram" => match seq::parse::parse(first, &statements[1..]) {
            Ok(sd) => Parsed::Seq(sd),
            Err(e) => Parsed::Failed(reason(e)),
        },
        _ => Parsed::Failed(format!("{} diagrams aren't drawn yet", crate::doc::clean_line(&pre::cut(keyword, 30)))),
    };
    (title, parsed)
}

fn reason(e: NotDrawn) -> String {
    match e {
        NotDrawn::Unsupported(why) => why,
        NotDrawn::TooLarge => TOO_LARGE.into(),
    }
}

/// The ladder for a flowchart drawn in `dir` (decision 10: a left-to-right
/// diagram tries every left-to-right step before turning).
pub fn ladder(dir: Dir) -> &'static [Step] {
    use Step::*;
    match dir.base() {
        Dir::LR => &[Normal, Compact, LrLabels16, LrLabels8, Turned, TurnedCompact, Labels16, Labels8],
        _ => &[Normal, Compact, Labels16, Labels8],
    }
}

/// Lay out a diagram for `req.width` columns.
pub fn render(source: &str, req: &Request, dg: &mut Diagrams) -> Outcome {
    let entry = dg.entry(source);
    let title = entry.title.clone();
    let width = req.width;
    let long = source.len() > MAX_SOURCE;
    let mut extra = Vec::new();
    match &mut entry.parsed {
        Parsed::Failed(why) => Outcome::Source(why.clone()),
        Parsed::Seq(sd) => {
            // Sequence diagrams are drawn in M4; until then, the transcript.
            let too_large = sd.too_large() || long;
            let d = outline_for(&mut entry.outlines, width, title, || {
                let mut d = outline::seq_transcript(sd, width);
                if too_large {
                    d.notes.push(TOO_LARGE.into());
                }
                d
            });
            Outcome::Outline(d)
        }
        Parsed::Flow { graph, prep } => {
            let visible = graph.nodes.iter().filter(|n| !n.dead).count();
            if visible > MAX_NODES || graph.edges.len() > MAX_EDGES || long {
                extra.push(TOO_LARGE.to_string());
            } else if !graph.clusters.is_empty() {
                extra.push("subgraphs aren't drawn yet".into());
            } else {
                let prep = prep.get_or_insert_with(|| flow::prep::prepare(graph, &mut entry.work));
                match prep {
                    Ok(prep) => {
                        for &step in ladder(graph.dir) {
                            let cached = entry.drawings.iter().find(|(s, c, _)| *s == step && *c == req.charset);
                            let result = match cached {
                                Some((_, _, r)) => r.clone(),
                                None => {
                                    let r = flow::draw::draw(graph, prep, step, req.charset, &mut entry.work)
                                        .map(|mut c| {
                                            c.drawing.title = title.clone();
                                            c.drawing.notes = flow_notes(graph, step, std::mem::take(&mut c.drawing.notes));
                                            (Arc::new(c.drawing), c.problems)
                                        });
                                    entry.drawings.push((step, req.charset, r.clone()));
                                    r
                                }
                            };
                            match result {
                                Ok((d, problems)) if !problems.any() && d.width <= width => {
                                    return Outcome::Drawn(d);
                                }
                                Ok(_) => {}
                                Err(NotDrawn::TooLarge) => {
                                    extra.push(TOO_LARGE.into());
                                    break;
                                }
                                Err(NotDrawn::Unsupported(why)) => return Outcome::Source(why),
                            }
                        }
                    }
                    Err(NotDrawn::TooLarge) => extra.push(TOO_LARGE.into()),
                    Err(NotDrawn::Unsupported(why)) => return Outcome::Source(why.clone()),
                }
            }
            let d = outline_for(&mut entry.outlines, width, title, || {
                let mut d = outline::flow_outline(graph, width);
                d.notes.extend(extra);
                d
            });
            Outcome::Outline(d)
        }
    }
}

/// The cached outline at `width`, or a new one from `make`.
fn outline_for(
    cache: &mut Vec<(usize, Arc<Drawing>)>,
    width: usize,
    title: Option<String>,
    make: impl FnOnce() -> Drawing,
) -> Arc<Drawing> {
    if let Some((_, d)) = cache.iter().find(|(w, _)| *w == width) {
        return d.clone();
    }
    let mut d = make();
    // A title written in the diagram itself (`title …` in a sequence
    // diagram) replaces the front-matter one, as in Mermaid.
    d.title = d.title.take().or(title);
    let d = Arc::new(d);
    cache.push((width, d.clone()));
    d
}

/// Notes for the bottom rule of a flowchart drawn at `step`: how its
/// direction changed (plan sections 6.1 and 7.1), then "simplified" when
/// part of it is drawn in a simpler form (plan section 3.3), then the notes
/// the drawing came with.
fn flow_notes(g: &FlowGraph, step: Step, drawn: Vec<String>) -> Vec<String> {
    let mut notes = Vec::new();
    match g.dir {
        Dir::LR | Dir::RL if step.turned() => notes.push("drawn top-down to fit".to_string()),
        Dir::RL => notes.push("drawn left to right".to_string()),
        Dir::BT => notes.push("drawn top-down".to_string()),
        Dir::TD | Dir::LR => {}
    }
    // The direction notes already cover "mirror", and "subgraph" only says
    // that the diagram has subgraphs.
    if g.simplified.iter().any(|s| s != "mirror" && s != "subgraph") {
        notes.push("simplified".to_string());
    }
    for note in drawn {
        if !notes.contains(&note) {
            notes.push(note);
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(src: &str) -> FlowGraph {
        let st = pre::statements(src);
        flow::parse::parse(&st[0], &st[1..]).expect("parses")
    }

    /// The outline or transcript `render` gave, or a panic.
    fn outline(src: &str, width: usize) -> Arc<Drawing> {
        match render(src, &Request { width, charset: Charset::Unicode }, &mut Diagrams::default()) {
            Outcome::Outline(d) => d,
            Outcome::Drawn(d) => panic!("drawn: {}", d.text()),
            Outcome::Source(why) => panic!("source: {why}"),
        }
    }

    #[test]
    fn notes_say_how_the_direction_changed() {
        let cases: [(&str, Step, &[&str]); 7] = [
            ("flowchart TD\nA-->B", Step::Normal, &[]),
            ("flowchart LR\nA-->B", Step::LrLabels8, &[]),
            ("flowchart LR\nA-->B", Step::Turned, &["drawn top-down to fit"]),
            ("flowchart RL\nA-->B", Step::Compact, &["drawn left to right"]),
            ("flowchart RL\nA-->B", Step::TurnedCompact, &["drawn top-down to fit"]),
            ("flowchart RL\nA-->B", Step::Labels8, &["drawn top-down to fit"]),
            ("flowchart BT\nA-->B", Step::Labels16, &["drawn top-down"]),
        ];
        for (src, step, want) in cases {
            assert_eq!(flow_notes(&graph(src), step, Vec::new()), want, "{src} at {step:?}");
        }
    }

    #[test]
    fn an_unknown_shape_is_noted_as_simplified() {
        let g = graph("flowchart RL\nA@{ shape: bolt } --> B");
        assert_eq!(flow_notes(&g, Step::Turned, Vec::new()), ["drawn top-down to fit", "simplified"]);
        assert_eq!(flow_notes(&g, Step::Normal, Vec::new()), ["drawn left to right", "simplified"]);
        let g = graph("flowchart TD\nA@{ shape: hex } --> B");
        assert!(flow_notes(&g, Step::Normal, Vec::new()).is_empty());
        // Notes the drawing came with follow, once each.
        let drawn = vec!["simplified".to_string(), "x".to_string(), "x".to_string()];
        let g = graph("flowchart BT\nA@{ shape: bolt }");
        assert_eq!(flow_notes(&g, Step::Normal, drawn), ["drawn top-down", "simplified", "x"]);
    }

    #[test]
    fn a_drawing_carries_its_notes() {
        let req = Request { width: 80, charset: Charset::Unicode };
        match render("flowchart BT\nA@{ shape: bolt } --> B", &req, &mut Diagrams::default()) {
            Outcome::Drawn(d) => assert_eq!(d.notes, ["drawn top-down", "simplified"]),
            _ => panic!("not drawn"),
        }
    }

    #[test]
    fn a_long_source_gets_its_outline() {
        let comments = "%% a comment that only makes the source longer\n".repeat(1500);
        let flow = format!("flowchart TD\nA[Start] --> B[End]\n{comments}");
        assert!(flow.len() > MAX_SOURCE && flow.len() <= MAX_PARSE);
        let d = outline(&flow, 60);
        assert_eq!(d.text(), "Start ─▸ End\n");
        assert_eq!(d.notes, [TOO_LARGE]);

        let seq = format!("sequenceDiagram\nAlice->>Bob: Hi\n{comments}");
        let d = outline(&seq, 60);
        assert_eq!(d.text(), "1. Alice ─▸ Bob: Hi\n");
        assert_eq!(d.notes, [TOO_LARGE]);

        // Past the parse limit the source isn't read at all.
        let huge = format!("flowchart TD\nA --> B\n{}", "%% filler\n".repeat(MAX_PARSE / 10));
        match render(&huge, &Request { width: 60, charset: Charset::Unicode }, &mut Diagrams::default()) {
            Outcome::Source(why) => assert_eq!(why, TOO_LARGE),
            _ => panic!("parsed past the limit"),
        }
    }
}
