//! The flowchart parser (plan section 3.3), ported from `parse_flow` in the
//! prototype.

use std::collections::{HashMap, HashSet};

use super::{Cluster, Dir, Edge, FlowGraph, Head, Node, Shape, Stroke};
use crate::diagram::pre::{Re, decode, is_alnum, is_id_char, quote, re};
use crate::diagram::{MAX_SOURCE, NotDrawn};

/// Statements that only style or describe the diagram. They are skipped.
/// (`title` lines are handled on their own, see [`is_title`].)
const IGNORED: &[&str] = &["classDef", "class", "style", "linkStyle", "click", "accTitle", "accDescr"];

/// A shape's opener, its closers and the shape. Longer openers come first.
const SHAPES: &[(&str, &[&str], Shape)] = &[
    ("(((", &[")))"], Shape::DCircle),
    ("((", &["))"], Shape::Circle),
    ("([", &["])"], Shape::Stadium),
    ("[[", &["]]"], Shape::Subroutine),
    ("[(", &[")]"], Shape::Cylinder),
    ("[/", &["/]", "\\]"], Shape::Slant),
    ("[\\", &["\\]", "/]"], Shape::Slant),
    ("{{", &["}}"], Shape::Hexagon),
    ("(", &[")"], Shape::Round),
    ("[", &["]"], Shape::Rect),
    ("{", &["}"], Shape::Decision),
    (">", &["]"], Shape::Asym),
];

/// The edges one diagram may have before it is too large to read. Groups
/// such as `A & B --> C & D` multiply, so this guards time and memory. It is
/// far above the 300 edges a drawing may have.
const MAX_PARSED_EDGES: usize = 5_000;

/// The bytes of edge labels one diagram may hold. A group link copies its
/// label into every edge it makes, so a short source could otherwise hold
/// gigabytes. Without groups the labels can't be longer than the source.
const MAX_EDGE_LABEL_BYTES: usize = 4 * MAX_SOURCE;

/// The steps [`Parser::finish`] may take to move the edges of subgraphs
/// used as edge ends. Real diagrams need a few thousand.
const MAX_FINISH_STEPS: usize = 20_000_000;

/// The steps the parser may take walking up nested subgraphs, to keep
/// subgraphs from nesting in themselves. Real diagrams need a few hundred;
/// thousands of deeply nested subgraphs could otherwise take seconds.
const MAX_NEST_STEPS: usize = 2_000_000;

/// `id [title]`. The title may be a Markdown string over several lines.
static SUBGRAPH_TITLE: Re = re!(r#"(?s)^([^\s\[\]"]+)\s*\[(.*)\]\s*$"#);
/// A statement that is only `id@{ … }`.
static META_ONLY: Re = re!(r"^([\p{L}\p{N}_-]+)@\{([^}]*)\}$");
/// One `key: value` of `@{ … }`. A value is quoted (`"…"`, or `'…'` with
/// `''` for a quote, as in YAML) or runs to a comma or a line break.
static META_KEY: Re = re!(r#"([\p{L}\p{N}_]+)\s*:\s*("[^"]*"|'(?:[^']|'')*'|[^,\n]+)"#);
static CLASS: Re = re!(r"^:::[\p{L}\p{N}_-]+");
static EDGE_ID: Re = re!(r"^[A-Za-z_][\p{L}\p{N}_-]*@");
/// A link without edge text. An invisible link (`~~~`) has no heads.
static LINK: Re = re!(r"^(?:([<ox])?(-{2,}|={2,}|-\.+-)([>ox])?|(~{3,}))");
static CLOSE_SOLID: Re = re!(r"(-{2,})([>ox]?)");
static CLOSE_THICK: Re = re!(r"(={2,})([>ox]?)");
static CLOSE_DOTTED: Re = re!(r"(\.+)-([>ox]?)");

/// A link between two groups of nodes, before it becomes edges.
#[derive(Clone)]
struct Link {
    stroke: Stroke,
    head_from: Head,
    head_to: Head,
    label: Option<String>,
    min_len: usize,
}

/// A subgraph whose `end` has not come yet.
struct Open {
    cluster: usize,
    /// The nodes its body has named so far.
    named: Vec<usize>,
    /// The subgraphs opened directly in its body.
    nested: Vec<usize>,
}

#[derive(Default)]
struct Parser {
    g: FlowGraph,
    /// Subgraph id → index into `g.clusters`.
    cluster_ids: HashMap<String, usize>,
    /// For each subgraph, whether its parent is settled: a subgraph that
    /// lists it has closed. In Mermaid the first one to close wins.
    taken: Vec<bool>,
    /// The open subgraphs, innermost last.
    open: Vec<Open>,
    /// Steps taken by [`inside`] so far.
    nest_steps: usize,
    /// Ids given to edges (`A e1@--> B`), so that `e1@{ … }` is not read as
    /// a node.
    edge_ids: HashSet<String>,
    /// Bytes of edge labels held so far.
    label_bytes: usize,
}

/// Parse a flowchart. `header` is the first statement (`flowchart LR`,
/// possibly followed by more text); `statements` are the rest, from
/// [`crate::diagram::pre::statements`].
pub fn parse(header: &str, statements: &[String]) -> Result<FlowGraph, NotDrawn> {
    let mut p = Parser::default();
    let words: Vec<&str> = header.split_whitespace().collect();
    let mut rest = "";
    if let Some(word) = words.get(1) {
        let word = word.trim_end_matches(';');
        if let Some(dir) = direction(word).or_else(|| arrow_direction(word)) {
            p.g.dir = dir;
            rest = after_words(header, 2);
        } else {
            rest = after_words(header, 1);
        }
    }
    if matches!(p.g.dir, Dir::BT | Dir::RL) {
        p.g.simplified.insert("mirror".into());
    }
    let mut todo: Vec<&str> = Vec::new();
    if !rest.trim().is_empty() {
        todo.push(rest);
    }
    todo.extend(statements.iter().map(String::as_str));
    if todo.is_empty() {
        return Err(NotDrawn::unsupported("empty diagram"));
    }

    let mut skipping = false;
    for st in todo {
        let st = st.trim();
        if skipping {
            skipping = !st.contains('}');
            continue;
        }
        let mut words = st.split_whitespace();
        let first = words.next().unwrap_or("");
        let word = &first[..first.bytes().take_while(u8::is_ascii_alphabetic).count()];
        if IGNORED.contains(&word) && matches!(first.as_bytes().get(word.len()), None | Some(b':' | b'{')) {
            // A multi-line `accDescr { … }` runs to its `}`; a brace in the
            // text of `accDescr: …` does not start one.
            let block = st[word.len()..].trim_start().starts_with('{');
            skipping = word == "accDescr" && block && !st.contains('}');
            continue;
        }
        if is_title(st) {
            continue;
        }
        if first == "subgraph" {
            let parent = p.open.last().map(|o| o.cluster);
            let k = p.subgraph(st["subgraph".len()..].trim(), parent)?;
            if let Some(o) = p.open.last_mut() {
                o.nested.push(k);
            }
            p.open.push(Open { cluster: k, named: Vec::new(), nested: Vec::new() });
            continue;
        }
        if st == "end" {
            if !p.close()? {
                return Err(NotDrawn::unsupported("\"end\" without a subgraph"));
            }
            continue;
        }
        // Mermaid reads `direction` as a keyword only before a direction;
        // otherwise it is a node id.
        if first == "direction" && words.next().is_some_and(|w| direction(w).is_some()) {
            if !p.open.is_empty() {
                p.g.simplified.insert("subgraph direction".into());
            }
            continue;
        }
        if p.meta_only(st)? {
            continue;
        }
        p.chain(st)?;
    }
    if !p.open.is_empty() {
        return Err(NotDrawn::unsupported("subgraph without \"end\""));
    }
    p.finish()
}

/// Whether `st` is a `title` line. Mermaid's flowcharts have no title, but
/// authors write `title Words` anyway, so such a line is skipped. When a
/// link or `&` follows the word, as in `title --> intro`, `title` is a node.
fn is_title(st: &str) -> bool {
    let Some(rest) = st.strip_prefix("title") else {
        return false;
    };
    if rest.starts_with(':') {
        return true;
    }
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return false;
    }
    let rest = rest.trim_start();
    let rest = EDGE_ID.find(rest).filter(|m| starts_link(&rest[m.end()..])).map_or(rest, |m| &rest[m.end()..]);
    !rest.starts_with('&') && !starts_link(rest)
}

fn direction(word: &str) -> Option<Dir> {
    match word {
        "TB" | "TD" => Some(Dir::TD),
        "BT" => Some(Dir::BT),
        "LR" => Some(Dir::LR),
        "RL" => Some(Dir::RL),
        _ => None,
    }
}

/// The arrows Mermaid also takes as a direction after `graph` or
/// `flowchart`, but not after `direction`: `v` is TB, `^` BT, `>` LR and
/// `<` RL.
fn arrow_direction(word: &str) -> Option<Dir> {
    match word {
        "v" => Some(Dir::TD),
        "^" => Some(Dir::BT),
        ">" => Some(Dir::LR),
        "<" => Some(Dir::RL),
        _ => None,
    }
}

/// The text after the first `n` words, without the space before it.
fn after_words(s: &str, n: usize) -> &str {
    let mut rest = s.trim_start();
    for _ in 0..n {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        rest = rest[end..].trim_start();
    }
    rest
}

/// Skip spaces and tabs.
fn skip(s: &str, i: usize) -> usize {
    i + s[i..].bytes().take_while(|&b| b == b' ' || b == b'\t').count()
}

/// The length of the node id at the start of `s`. `-` and `.` may join two
/// parts of an id, as in `api-v2` or `db.main`.
fn read_id(s: &str) -> usize {
    let mut chars = s.char_indices().peekable();
    let mut end = 0;
    while let Some((k, c)) = chars.next() {
        let joins = (c == '-' || c == '.')
            && k > 0
            && chars.peek().is_some_and(|&(_, n)| is_alnum(n) || n == '_');
        if !is_id_char(c) && !joins {
            break;
        }
        end = k + c.len_utf8();
    }
    end
}

fn head(c: Option<char>) -> Head {
    match c {
        Some('<' | '>') => Head::Arrow,
        Some('o') => Head::Circle,
        Some('x') => Head::Cross,
        _ => Head::None,
    }
}

/// Whether `s` starts with a link, as edge ids require (`e1@-->`).
fn starts_link(s: &str) -> bool {
    let link = |s: &str| ["--", "==", "-.", "~~~"].iter().any(|op| s.starts_with(op));
    link(s) || (s.starts_with(['<', 'o', 'x']) && link(&s[1..]))
}

/// Whether the character `c` after `op` (`--`, `==` or `-.`) starts edge
/// text, as in `A -- text --> B` or `A -.text.-> B`. After `--` and `==` an
/// `o` or `x` is a head (`A --o B`). After `-.` it can only be text, since a
/// dotted link needs a `-` after its dots (`A -.optional.-> B`).
fn starts_edge_text(op: &str, c: Option<char>) -> bool {
    let not_text = if op == "-." { ".-|" } else { "-.>=ox|" };
    match c {
        Some(' ' | '\t' | '"') => true,
        Some(c) => !not_text.contains(c) && !c.is_whitespace(),
        None => false,
    }
}

/// The `--`, `==` or `-.` at the start of `s` when edge text follows it.
fn edge_text_op(s: &str) -> Option<&'static str> {
    ["--", "==", "-."].into_iter().find(|op| s.starts_with(op) && starts_edge_text(op, s[2..].chars().next()))
}

/// The regex for the arrow that closes edge text opened by `op`.
fn closer(op: &str) -> &'static Re {
    match op {
        "--" => &CLOSE_SOLID,
        "==" => &CLOSE_THICK,
        _ => &CLOSE_DOTTED,
    }
}

/// The index after the quoted text (`"…"` or a Markdown string ``"`…`"``)
/// whose opening quote is at `q`, or `None` if nothing closes it.
fn quote_end(s: &str, q: usize) -> Option<usize> {
    let md = s[q + 1..].starts_with('`').then(|| s[q + 2..].find("`\"").map(|e| q + 2 + e + 2)).flatten();
    md.or_else(|| s[q + 1..].find('"').map(|e| q + 2 + e))
}

/// The arrow that closes edge text opened by `op`, searched from byte
/// `start` of `s`. Quoted text is skipped, since Mermaid reads it as one
/// string: `A -- "git push --force" --> B`. A quote that nothing closes is
/// read as text. Each byte is looked at a bounded number of times.
fn close_text<'a>(s: &'a str, start: usize, op: &str) -> Option<regex::Captures<'a>> {
    let re = closer(op);
    let mut from = start;
    loop {
        let q = s[from..].find('"').map(|q| from + q);
        // A closer never holds a quote, so one before `q` ends there.
        if let Some(c) = re.captures_at(&s[..q.unwrap_or(s.len())], from) {
            return Some(c);
        }
        let q = q?;
        match quote_end(s, q) {
            Some(end) => from = end,
            None => return re.captures_at(s, q),
        }
    }
}

/// Where the link at byte `k` of `line` ends, edge text included
/// (`-- a;b -->`), or `None` if no link starts there. Statements are split
/// with this, since Mermaid reads a `;` inside edge text as text.
/// `unclosed` notes each opener (`--`, `==`, `-.`) that nothing closes
/// after some point of the line, so a line of many openers is read once.
pub(crate) fn link_end(line: &str, k: usize, unclosed: &mut [bool; 3]) -> Option<usize> {
    let s = &line[k..];
    if let Some(op) = edge_text_op(s) {
        let n = ["--", "=="].iter().position(|&o| o == op).unwrap_or(2);
        if unclosed[n] {
            return None;
        }
        let end = close_text(line, k + 2, op).map(|c| c.get(0).expect("whole match").end());
        unclosed[n] = end.is_none();
        return end;
    }
    LINK.find(s).map(|m| k + m.end())
}

/// The `|text|` after a link, from byte `j` of `s`: the text and the index
/// after the closing `|`. A `|` inside quotes is text (`-->|"a|b"|`); if
/// quotes leave nothing to close it, the first `|` does.
fn pipe_text(s: &str, j: usize) -> Option<(&str, usize)> {
    let t = s.len() - s[j..].trim_start().len();
    if !s[t..].starts_with('|') {
        return None;
    }
    let mut quoted = false;
    let mut end = None;
    for (p, b) in s.bytes().enumerate().skip(t + 1) {
        match b {
            b'"' => quoted = !quoted,
            b'|' if !quoted => {
                end = Some(p);
                break;
            }
            _ => {}
        }
    }
    let end = end.or_else(|| s[t + 1..].find('|').map(|e| t + 1 + e))?;
    Some((&s[t + 1..end], end + 1))
}

/// The text of a node shape that starts at `i`, the index after its
/// closer, and the closer.
fn shape_text<'a>(s: &'a str, i: usize, closers: &[&'static str]) -> Result<(&'a str, usize, &'static str), NotDrawn> {
    let t = i + s[i..].bytes().take_while(|&b| b == b' ').count();
    if s[t..].starts_with('"') {
        let Some(e) = s[t + 1..].find('"').map(|e| t + 1 + e) else {
            return Err(NotDrawn::unsupported("unterminated quote"));
        };
        let after = e + 1 + s[e + 1..].bytes().take_while(|&b| b == b' ').count();
        for &c in closers {
            if s[after..].starts_with(c) {
                return Ok((&s[t..=e], after + c.len(), c));
            }
        }
        return Err(NotDrawn::unsupported("text after a quoted label"));
    }
    // The nearest closer wins (the first listed, at one place). One scan
    // finds it, so a closer that never comes is not looked for to the end
    // of the statement for every node (`A[/x/] & A[/x/] & …`).
    let starts: Vec<char> = closers.iter().filter_map(|c| c.chars().next()).collect();
    let best = s[i..]
        .match_indices(|ch| starts.contains(&ch))
        .find_map(|(p, _)| closers.iter().find(|&&c| s[i + p..].starts_with(c)).map(|&c| (i + p, c)));
    match best {
        Some((e, c)) => Ok((&s[i..e], e + c.len(), c)),
        None => Err(NotDrawn::unsupported("unterminated node shape")),
    }
}

/// The shape a `@{ shape: … }` name stands for, with its side glyphs.
fn named_shape(name: &str) -> Option<(Shape, Option<(char, char)>)> {
    let shape = match name {
        "rect" | "proc" | "process" | "rectangle" => Shape::Rect,
        "rounded" | "event" => Shape::Round,
        "stadium" | "pill" | "terminal" => Shape::Stadium,
        "subproc" | "subprocess" | "subroutine" | "fr-rect" => Shape::Subroutine,
        "cyl" | "cylinder" | "database" | "db" => Shape::Cylinder,
        "circle" | "circ" => Shape::Circle,
        "dbl-circ" | "double-circle" => Shape::DCircle,
        "diamond" | "diam" | "decision" | "question" => Shape::Decision,
        "hex" | "hexagon" | "prepare" => Shape::Hexagon,
        "lean-r" => return Some((Shape::Slant, Some(('/', '/')))),
        "lean-l" => return Some((Shape::Slant, Some(('\\', '\\')))),
        "trap-b" => return Some((Shape::Slant, Some(('/', '\\')))),
        "trap-t" => return Some((Shape::Slant, Some(('\\', '/')))),
        "text" => Shape::Bare,
        _ => return None,
    };
    Some((shape, None))
}

/// Whether subgraph `c` is `d` or nested in it. The walk is bounded, so it
/// ends even on a cycle. `steps` counts the steps taken.
fn inside(clusters: &[Cluster], c: usize, d: usize, steps: &mut usize) -> bool {
    let mut up = Some(c);
    for _ in 0..=clusters.len() {
        *steps += 1;
        match up {
            Some(u) if u == d => return true,
            Some(u) => up = clusters[u].parent,
            None => return false,
        }
    }
    false
}

/// Put the subgraphs in tree order: each one followed by those nested in
/// it, siblings in declaration order. Outlines list them in this order,
/// indented by depth, so a subgraph nested by name, which can be declared
/// before its parent, is shown under it. Indexes are renumbered.
fn tree_order(g: &mut FlowGraph) {
    let n = g.clusters.len();
    let mut children = vec![Vec::new(); n];
    let mut roots = Vec::new();
    for (c, cl) in g.clusters.iter().enumerate() {
        match cl.parent {
            Some(p) if p < n && p != c => children[p].push(c),
            _ => roots.push(c),
        }
    }
    let mut order = Vec::with_capacity(n);
    let mut seen = vec![false; n];
    let mut todo: Vec<usize> = roots.into_iter().rev().collect();
    while let Some(c) = todo.pop() {
        if !seen[c] {
            seen[c] = true;
            order.push(c);
            todo.extend(children[c].iter().rev());
        }
    }
    // Only a cycle could leave some out; keep them all the same.
    order.extend((0..n).filter(|&c| !seen[c]));
    if order.iter().enumerate().all(|(i, &c)| i == c) {
        return;
    }
    let mut new = vec![0; n];
    for (i, &c) in order.iter().enumerate() {
        new[c] = i;
    }
    let mut old: Vec<Option<Cluster>> = std::mem::take(&mut g.clusters).into_iter().map(Some).collect();
    g.clusters = order.iter().filter_map(|&c| old[c].take()).collect();
    let renumber = |c: &mut Option<usize>| *c = c.map(|c| new[c]);
    for cl in &mut g.clusters {
        renumber(&mut cl.parent);
    }
    for node in &mut g.nodes {
        renumber(&mut node.cluster);
    }
    for e in &mut g.edges {
        renumber(&mut e.from_cluster);
        renumber(&mut e.to_cluster);
    }
}

/// Where the `}` that ends `@{ … }` shape data is, searching from byte
/// `from`. A `}` inside double quotes is text, as in Mermaid; if quotes
/// leave nothing to end it, the first `}` does.
fn meta_end(s: &str, from: usize) -> Option<usize> {
    let mut quoted = false;
    for (p, b) in s.bytes().enumerate().skip(from) {
        match b {
            b'"' => quoted = !quoted,
            b'}' if !quoted => return Some(p),
            _ => {}
        }
    }
    s[from..].find('}').map(|e| from + e)
}

/// A shape-data value as YAML reads it: one pair of quotes removed, `''`
/// as one quote inside single quotes, and line breaks inside quotes joined
/// with a space. An unquoted value loses stray double quotes.
fn meta_value(v: &str) -> String {
    let v = v.trim();
    let fold = |t: &str| t.split('\n').map(str::trim).collect::<Vec<_>>().join(" ");
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        fold(&v[1..v.len() - 1])
    } else if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        fold(&v[1..v.len() - 1].replace("''", "'"))
    } else {
        v.trim_matches('"').to_string()
    }
}

/// The `key: value` pairs of shape data, values read by [`meta_value`].
/// Pairs are separated by commas or line breaks.
///
/// Shape data over several lines is YAML, so there `#` starts a comment,
/// and the data may hold nothing but pairs and comments: a statement that
/// a `}` far below swallowed would otherwise lose its text silently.
fn meta_pairs(meta: &str) -> Result<Vec<(&str, String)>, NotDrawn> {
    let lines = meta.contains('\n');
    // Text between pairs: commas, spaces and comments only.
    let check = |gap: &str| {
        let code = |l: &str| l.split('#').next().unwrap_or("").chars().all(|c| c.is_whitespace() || c == ',');
        if gap.split('\n').all(code) {
            Ok(())
        } else {
            Err(NotDrawn::unsupported(format!("unexpected text {} in @{{ }}", quote(gap.trim()))))
        }
    };
    let mut pairs = Vec::new();
    let mut last = 0;
    for c in META_KEY.captures_iter(meta) {
        let m = c.get(0).expect("whole match");
        let mut value = c.get(2).expect("value").as_str();
        if lines {
            let gap = &meta[last..m.start()];
            if gap.rsplit('\n').next().is_some_and(|l| l.contains('#')) {
                continue; // a pair inside a comment
            }
            check(gap)?;
            if !value.starts_with(['"', '\'']) {
                let comment = value.char_indices().find(|&(k, c)| c == '#' && (k == 0 || value[..k].ends_with([' ', '\t'])));
                value = &value[..comment.map_or(value.len(), |(k, _)| k)];
            }
        }
        last = m.end();
        pairs.push((c.get(1).expect("key").as_str(), meta_value(value)));
    }
    if lines {
        check(&meta[last..])?;
    }
    Ok(pairs)
}

impl Parser {
    /// Open a subgraph from the text after `subgraph`; returns its index.
    fn subgraph(&mut self, rest: &str, parent: Option<usize>) -> Result<usize, NotDrawn> {
        let (id, title) = if let Some(c) = SUBGRAPH_TITLE.captures(rest) {
            (c[1].to_string(), decode(&c[2]))
        } else if rest.starts_with('"') {
            let t = decode(rest);
            (t.clone(), t)
        } else {
            (rest.to_string(), decode(rest))
        };
        if id.is_empty() {
            return Err(NotDrawn::unsupported("subgraph without a name"));
        }
        let clusters = &mut self.g.clusters;
        if let Some(&k) = self.cluster_ids.get(&id) {
            // Reopened: the last title wins. So does the parent, until a
            // subgraph that lists it has closed, and never to make a cycle.
            clusters[k].title = title;
            if !self.taken[k] && !parent.is_some_and(|p| inside(clusters, p, k, &mut self.nest_steps)) {
                clusters[k].parent = parent;
            }
            if self.nest_steps > MAX_NEST_STEPS {
                return Err(NotDrawn::TooLarge);
            }
            return Ok(k);
        }
        // Mermaid lists a subgraph in its parent by id, like a node. So if a
        // subgraph that closed earlier took a node of this name, the new
        // subgraph is nested there.
        let named_in = self.g.ids.get(&id).and_then(|&n| self.g.nodes[n].cluster);
        let k = clusters.len();
        clusters.push(Cluster { id: id.clone(), title, parent: named_in.or(parent), members: Vec::new() });
        self.taken.push(named_in.is_some());
        self.cluster_ids.insert(id, k);
        self.g.simplified.insert("subgraph".into());
        Ok(k)
    }

    /// Close the innermost subgraph; `false` if none is open. It takes every
    /// node its body named that no subgraph closed earlier has taken, as
    /// Mermaid does. So an inner subgraph wins over an outer one, and naming
    /// a node outside every subgraph does not keep it out of one. Members
    /// are in the order the body first names them.
    ///
    /// Subgraphs are taken the same way: those opened in the body, and
    /// those whose id the body names (`subgraph Outer / Inner / end`), as
    /// Mermaid lists both by id.
    fn close(&mut self) -> Result<bool, NotDrawn> {
        let Some(Open { cluster: c, named, nested }) = self.open.pop() else {
            return Ok(false);
        };
        for d in nested {
            // Unless a subgraph that closed earlier named it.
            if self.g.clusters[d].parent == Some(c) {
                self.taken[d] = true;
            }
        }
        for k in named {
            if self.g.nodes[k].cluster.is_some() {
                continue;
            }
            self.g.nodes[k].cluster = Some(c);
            self.g.clusters[c].members.push(k);
            if let Some(&d) = self.cluster_ids.get(&self.g.nodes[k].id)
                && !self.taken[d]
                && !inside(&self.g.clusters, c, d, &mut self.nest_steps)
            {
                self.g.clusters[d].parent = Some(c);
                self.taken[d] = true;
            }
            if self.nest_steps > MAX_NEST_STEPS {
                return Err(NotDrawn::TooLarge);
            }
        }
        Ok(true)
    }

    /// Handle a statement that is only `id@{ … }`; `true` if it is skipped.
    /// Edge settings (`e1@{ animate: true }`) are skipped, a collapsed
    /// subgraph falls back to source, and node settings are left to
    /// [`Parser::chain`]. Only the keys are looked at, never label text.
    fn meta_only(&self, st: &str) -> Result<bool, NotDrawn> {
        let Some(c) = META_ONLY.captures(st) else {
            return Ok(false);
        };
        let mut edge = self.edge_ids.contains(&c[1]);
        for kv in META_KEY.captures_iter(&c[2]) {
            match &kv[1] {
                "animate" | "animation" | "curve" => edge = true,
                "view" if meta_value(&kv[2]) == "collapsed" => {
                    return Err(NotDrawn::unsupported("collapsed subgraphs"));
                }
                _ => {}
            }
        }
        Ok(edge)
    }

    /// A chain of groups joined by links: `A & B --> C -.-> D`.
    fn chain(&mut self, st: &str) -> Result<(), NotDrawn> {
        let (mut left, mut i) = self.group(st, 0)?;
        loop {
            i = skip(st, i);
            if i >= st.len() {
                return Ok(());
            }
            let Some((link, j)) = self.link(st, i)? else {
                return Err(NotDrawn::unsupported(format!("unexpected text {}", quote(&st[i..]))));
            };
            let (right, j) = self.group(st, j)?;
            let new = left.len().saturating_mul(right.len());
            let label_len = link.label.as_ref().map_or(0, String::len);
            self.label_bytes = self.label_bytes.saturating_add(new.saturating_mul(label_len));
            if self.g.edges.len().saturating_add(new) > MAX_PARSED_EDGES || self.label_bytes > MAX_EDGE_LABEL_BYTES {
                return Err(NotDrawn::TooLarge);
            }
            for &from in &left {
                for &to in &right {
                    self.g.edges.push(Edge {
                        from,
                        to,
                        stroke: link.stroke,
                        head_from: link.head_from,
                        head_to: link.head_to,
                        label: link.label.clone(),
                        min_len: link.min_len,
                        from_cluster: None,
                        to_cluster: None,
                    });
                }
            }
            left = right;
            i = j;
        }
    }

    /// Nodes joined by `&`, starting at `i`; returns them and the index
    /// after the last one.
    fn group(&mut self, s: &str, i: usize) -> Result<(Vec<usize>, usize), NotDrawn> {
        let mut ks = Vec::new();
        let mut i = i;
        loop {
            let (k, j) = self.node(s, i)?;
            ks.push(k);
            let t = skip(s, j);
            if s[t..].starts_with('&') {
                i = t + 1;
                continue;
            }
            return Ok((ks, j));
        }
    }

    /// The node named `id`, made if it is new. The open subgraph, if any,
    /// notes that its body named it.
    fn get_node(&mut self, id: &str) -> Result<usize, NotDrawn> {
        if id == "end" {
            return Err(NotDrawn::unsupported("lowercase \"end\" used as a node name"));
        }
        let k = match self.g.ids.get(id) {
            Some(&k) => k,
            None => {
                let k = self.g.nodes.len();
                self.g.ids.insert(id.to_string(), k);
                self.g.nodes.push(Node {
                    id: id.to_string(),
                    label: id.to_string(),
                    shape: Shape::Rect,
                    cluster: None,
                    slant: None,
                    explicit: false,
                    dead: false,
                });
                k
            }
        };
        if let Some(o) = self.open.last_mut()
            && o.named.last() != Some(&k)
        {
            o.named.push(k);
        }
        Ok(k)
    }

    /// One node with its optional shape or `@{ … }` and `:::class`.
    fn node(&mut self, s: &str, i: usize) -> Result<(usize, usize), NotDrawn> {
        let i = skip(s, i);
        let mut j = i + read_id(&s[i..]);
        if j == i {
            return Err(NotDrawn::unsupported(format!("expected a node at {}", quote(&s[i..]))));
        }
        let k = self.get_node(&s[i..j])?;
        if s[j..].starts_with("@{") {
            let Some(e) = meta_end(s, j + 2) else {
                return Err(NotDrawn::unsupported("unterminated @{"));
            };
            let meta = &s[j + 2..e];
            j = e + 1;
            for (key, value) in meta_pairs(meta)? {
                match key {
                    "shape" => {
                        let (shape, slant) = named_shape(&value).unwrap_or_else(|| {
                            self.g.simplified.insert("shape".into());
                            (Shape::Rect, None)
                        });
                        let node = &mut self.g.nodes[k];
                        node.shape = shape;
                        node.slant = slant;
                        node.explicit = true;
                    }
                    "label" => {
                        let node = &mut self.g.nodes[k];
                        node.label = decode(&format!("\"{value}\""));
                        node.explicit = true;
                    }
                    _ => {}
                }
            }
        } else if let Some(&(op, closers, shape)) = SHAPES.iter().find(|(op, _, _)| s[j..].starts_with(op)) {
            let (text, after, closer) = shape_text(s, j + op.len(), closers)?;
            let node = &mut self.g.nodes[k];
            node.label = decode(text);
            node.shape = shape;
            node.explicit = true;
            node.slant = match shape {
                Shape::Slant => op[1..].chars().next().zip(closer.chars().next()),
                Shape::Asym => Some(('>', '│')),
                _ => None,
            };
            j = after;
        }
        if s[j..].starts_with(":::") {
            j += CLASS.find(&s[j..]).map_or(3, |m| m.end());
        }
        Ok((k, j))
    }

    /// The link at `i`, or `None` if there is none; with the index after
    /// it and its `|label|`.
    fn link(&mut self, s: &str, i: usize) -> Result<Option<(Link, usize)>, NotDrawn> {
        let mut i = skip(s, i);
        if let Some(m) = EDGE_ID.find(&s[i..])
            && starts_link(&s[i + m.end()..])
        {
            self.edge_ids.insert(s[i..i + m.end() - 1].to_string());
            i += m.end();
        }
        // Edge text between the stroke's two halves: `A -- text --> B`, with
        // an optional head first (`A <-- text --> B`, `A o-- text --o B`).
        let lead = s[i..].starts_with(['<', 'o', 'x']) && edge_text_op(&s[i + 1..]).is_some();
        let at = i + usize::from(lead);
        if let Some(op) = edge_text_op(&s[at..]) {
            let start = at + 2;
            let Some(c) = close_text(s, start, op) else {
                return Err(NotDrawn::unsupported("edge text without a closing arrow"));
            };
            let all = c.get(0).expect("whole match");
            let right = c[2].chars().next();
            let len = c[1].len();
            let (stroke, min_len) = match op {
                "-." => (Stroke::Dotted, len),
                "==" => (Stroke::Thick, (len + usize::from(right.is_some())).saturating_sub(2).max(1)),
                _ => (Stroke::Solid, (len + usize::from(right.is_some())).saturating_sub(2).max(1)),
            };
            let label = Some(decode(&s[start..all.start()])).filter(|l| !l.is_empty());
            let head_from = if lead { head(s[i..].chars().next()) } else { Head::None };
            return Ok(Some((Link { stroke, head_from, head_to: head(right), label, min_len }, all.end())));
        }
        let Some(c) = LINK.captures(&s[i..]) else {
            return Ok(None);
        };
        let left = c.get(1).and_then(|m| m.as_str().chars().next());
        let right = c.get(3).and_then(|m| m.as_str().chars().next());
        let long = |len: usize| (len + usize::from(right.is_some())).saturating_sub(2).max(1);
        let (stroke, min_len) = if let Some(tildes) = c.get(4) {
            (Stroke::Invisible, tildes.len().saturating_sub(2).max(1))
        } else if c[2].starts_with('=') {
            (Stroke::Thick, long(c[2].len()))
        } else if c[2].contains('.') {
            (Stroke::Dotted, c[2].matches('.').count())
        } else {
            (Stroke::Solid, long(c[2].len()))
        };
        let mut j = i + c.get(0).expect("whole match").end();
        let mut label = None;
        if let Some((text, end)) = pipe_text(s, j) {
            label = Some(decode(text)).filter(|l| !l.is_empty());
            j = end;
        }
        Ok(Some((Link { stroke, head_from: head(left), head_to: head(right), label, min_len }, j)))
    }

    /// Move edges that end at a subgraph to one of its members, and check
    /// that something is left to draw.
    fn finish(mut self) -> Result<FlowGraph, NotDrawn> {
        let g = &mut self.g;
        // A node named like a subgraph, and given no shape or label, stands
        // for the subgraph and is not drawn. Its edges move to the first
        // member of the subgraph, or of a subgraph nested in it, that has no
        // edge from inside (simplified). The edge keeps the subgraph's index
        // so outlines can name the subgraph.
        let stand_ins: Vec<(usize, usize)> = (0..g.clusters.len())
            .filter_map(|c| g.ids.get(&g.clusters[c].id).map(|&k| (c, k)))
            .filter(|&(_, k)| !g.nodes[k].explicit)
            .collect();
        let steps = stand_ins.len().saturating_mul(g.clusters.len() + g.nodes.len() + g.edges.len());
        if steps > MAX_FINISH_STEPS {
            return Err(NotDrawn::TooLarge);
        }
        let mut hidden = vec![false; g.nodes.len()];
        for &(_, k) in &stand_ins {
            hidden[k] = true;
        }
        let mut children = vec![Vec::new(); g.clusters.len()];
        for (c, cl) in g.clusters.iter().enumerate() {
            if let Some(parent) = cl.parent {
                children[parent].push(c);
            }
        }
        for &(c, k) in &stand_ins {
            // The members of c, then those of the subgraphs nested in it.
            let mut members = Vec::new();
            let mut seen = vec![false; g.clusters.len()];
            let mut todo = vec![c];
            while let Some(d) = todo.pop() {
                if !seen[d] {
                    seen[d] = true;
                    members.extend(g.clusters[d].members.iter().copied().filter(|&m| !hidden[m]));
                    todo.extend(children[d].iter().rev());
                }
            }
            let Some(&first) = members.first() else {
                return Err(NotDrawn::unsupported("edge to an empty subgraph"));
            };
            let mut inside = vec![false; g.nodes.len()];
            for &m in &members {
                inside[m] = true;
            }
            let mut fed = vec![false; g.nodes.len()];
            for e in &g.edges {
                if inside[e.from] {
                    fed[e.to] = true;
                }
            }
            let target = members.iter().copied().find(|&m| !fed[m]).unwrap_or(first);
            for e in &mut g.edges {
                if e.from == k {
                    e.from = target;
                    e.from_cluster = Some(c);
                }
                if e.to == k {
                    e.to = target;
                    e.to_cluster = Some(c);
                }
            }
            g.nodes[k].dead = true;
            g.simplified.insert("subgraph edge".into());
        }
        if g.nodes.iter().all(|n| n.dead) {
            return Err(NotDrawn::unsupported("nothing to draw"));
        }
        tree_order(g);
        Ok(self.g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::pre;

    fn flow(src: &str) -> Result<FlowGraph, NotDrawn> {
        let (_, body) = pre::preamble(src).unwrap();
        let st = pre::statements(&body);
        parse(&st[0], &st[1..])
    }

    fn ok(src: &str) -> FlowGraph {
        flow(src).unwrap_or_else(|e| panic!("{src:?}: {e:?}"))
    }

    fn why(src: &str) -> String {
        match flow(src) {
            Err(NotDrawn::Unsupported(why)) => why,
            other => panic!("{src:?}: {other:?}"),
        }
    }

    fn ids(g: &FlowGraph) -> Vec<&str> {
        g.nodes.iter().map(|n| n.id.as_str()).collect()
    }

    fn edges(g: &FlowGraph) -> Vec<(&str, &str)> {
        g.edges.iter().map(|e| (g.nodes[e.from].id.as_str(), g.nodes[e.to].id.as_str())).collect()
    }

    #[test]
    fn header_sets_the_direction() {
        assert_eq!(ok("flowchart LR\nA").dir, Dir::LR);
        assert_eq!(ok("graph TB\nA").dir, Dir::TD);
        assert_eq!(ok("flowchart-elk RL\nA").dir, Dir::RL);
        assert!(ok("graph BT\nA").simplified.contains("mirror"));
        assert_eq!(edges(&ok("graph LR A-->B")), [("A", "B")]);
        assert_eq!(ok("flowchart\nA").dir, Dir::TD);
    }

    #[test]
    fn arrows_after_the_keyword_are_directions() {
        let g = ok("graph v\nA --> B");
        assert_eq!((g.dir, ids(&g)), (Dir::TD, vec!["A", "B"]));
        assert_eq!(ok("graph >\nA --> B").dir, Dir::LR);
        assert_eq!(ok("flowchart ^\nA --> B").dir, Dir::BT);
        let g = ok("graph <;A --> B");
        assert_eq!((g.dir, ids(&g)), (Dir::RL, vec!["A", "B"]));
        assert!(g.simplified.contains("mirror"));
        // Only after the keyword: elsewhere `v` is a node id, and the
        // `direction` statement takes only TB, TD, BT, RL and LR.
        assert_eq!(edges(&ok("flowchart LR\nv --> w")), [("v", "w")]);
        assert_eq!(why("flowchart LR\ndirection v"), "unexpected text \"v\"");
        assert_eq!(why("flowchart LR\ndirection >"), "unexpected text \">\"");
    }

    #[test]
    fn presentational_lines_never_make_nodes() {
        let g = ok("flowchart TD
            A --> B
            style C fill:#f9f
            classDef big font-size:20px
            class D,E big
            linkStyle 0 stroke:red
            click F callback \"Tip\"
            click G href \"https://example.com\"
            accTitle: T
            accDescr: D
            title Words");
        assert_eq!(ids(&g), ["A", "B"]);
        let g = ok("flowchart TD\nA:::big --> B\nclass1 --> style2");
        assert_eq!(ids(&g), ["A", "B", "class1", "style2"]);
        // A brace in one-line text does not start a block.
        let g = ok("flowchart TD\naccDescr: uses {braces; and more\nA --> B\naccDescr {\nx --> y\n}\nC");
        assert_eq!(ids(&g), ["A", "B", "C"]);
    }

    #[test]
    fn title_and_direction_are_node_ids_before_a_link() {
        let g = ok("flowchart TD\ntitle --> intro --> body\ndirection & speed --> velocity");
        assert_eq!(edges(&g), [("title", "intro"), ("intro", "body"), ("direction", "velocity"), ("speed", "velocity")]);
        // Only `direction` and a direction is the keyword.
        let g = ok("flowchart TD\nsubgraph S\ndirection LR\na\nend\ndirection TB\ndirection\ntitle My chart\ntitle: T\ntitle");
        assert_eq!(ids(&g), ["a", "direction"]);
        assert!(g.simplified.contains("subgraph direction"));
        let g = ok("flowchart TD\ntitle e1@--> x\ntitle[Big] --> y");
        assert_eq!(edges(&g), [("title", "x"), ("title", "y")]);
        assert_eq!(g.nodes[0].label, "Big");
    }

    #[test]
    fn o_and_x_after_a_link_are_heads() {
        let g = ok("flowchart LR\nA---oB\nC--xD\nE --- oF\nG-->x\nH o--o I\nJ x--x K");
        assert_eq!(ids(&g), ["A", "B", "C", "D", "E", "oF", "G", "x", "H", "I", "J", "K"]);
        let heads: Vec<(Head, Head)> = g.edges.iter().map(|e| (e.head_from, e.head_to)).collect();
        assert_eq!(
            heads,
            [
                (Head::None, Head::Circle),
                (Head::None, Head::Cross),
                (Head::None, Head::None),
                (Head::None, Head::Arrow),
                (Head::Circle, Head::Circle),
                (Head::Cross, Head::Cross),
            ]
        );
    }

    #[test]
    fn lowercase_end_is_not_a_node_name() {
        assert_eq!(why("flowchart TD\nA --> end"), "lowercase \"end\" used as a node name");
        assert_eq!(ids(&ok("flowchart TD\nA --> End --> END")), ["A", "End", "END"]);
        assert_eq!(why("flowchart TD\nend"), "\"end\" without a subgraph");
    }

    #[test]
    fn last_label_wins() {
        let g = ok("flowchart TD\nA[One] --> B\nA\nA(Two)");
        assert_eq!(g.nodes[0].label, "Two");
        assert_eq!(g.nodes[0].shape, Shape::Round);
        assert!(g.nodes[0].explicit);
        assert!(!g.nodes[1].explicit);
    }

    #[test]
    fn edge_text_with_and_without_spaces() {
        let g = ok("flowchart LR
            A -.tests.-> B
            C --text--> D
            E == big ==> F
            G -- plain --- H
            I -->|yes| J
            K -- \"quoted\" --> L
            M <-- back --> N");
        let labels: Vec<_> = g.edges.iter().map(|e| e.label.as_deref().unwrap()).collect();
        assert_eq!(labels, ["tests", "text", "big", "plain", "yes", "quoted", "back"]);
        let strokes: Vec<_> = g.edges.iter().map(|e| e.stroke).collect();
        use Stroke::*;
        assert_eq!(strokes, [Dotted, Solid, Thick, Solid, Solid, Solid, Solid]);
        assert_eq!(g.edges[3].head_to, Head::None);
        assert_eq!((g.edges[6].head_from, g.edges[6].head_to), (Head::Arrow, Head::Arrow));
        assert_eq!(why("flowchart LR\nA -- B"), "edge text without a closing arrow");
    }

    #[test]
    fn quoted_edge_text_may_hold_link_characters() {
        let g = ok("flowchart LR
            A -- \"git push --force\" --> B
            C -->|\"a|b\"| D
            E == \"x == y\" ==> F
            G -. \"a .- b\" .-> H
            I --\"x\"--> J
            K -- \"`**a** --> b`\" --> L");
        assert_eq!(edges(&g), [("A", "B"), ("C", "D"), ("E", "F"), ("G", "H"), ("I", "J"), ("K", "L")]);
        let labels: Vec<_> = g.edges.iter().map(|e| e.label.as_deref().unwrap()).collect();
        assert_eq!(labels, ["git push --force", "a|b", "x == y", "a .- b", "x", "a --> b"]);
        use Stroke::*;
        let strokes: Vec<_> = g.edges.iter().map(|e| e.stroke).collect();
        assert_eq!(strokes, [Solid, Solid, Thick, Dotted, Solid, Solid]);
        // A quote that nothing closes is text, as before.
        let g = ok("flowchart LR\nA -- 12\" pipe --> B\nC -->|12\" pipe| D");
        let labels: Vec<_> = g.edges.iter().map(|e| e.label.as_deref().unwrap()).collect();
        assert_eq!(labels, ["12\" pipe", "12\" pipe"]);
    }

    #[test]
    fn edge_text_after_a_circle_or_cross_head() {
        let g = ok("flowchart LR\nA o-- uses --o B\nC x-- blocks --x D\nE o-- two words --o F\nG x--x H");
        assert_eq!(edges(&g), [("A", "B"), ("C", "D"), ("E", "F"), ("G", "H")]);
        let labels: Vec<_> = g.edges.iter().map(|e| e.label.as_deref()).collect();
        assert_eq!(labels, [Some("uses"), Some("blocks"), Some("two words"), None]);
        let heads: Vec<_> = g.edges.iter().map(|e| (e.head_from, e.head_to)).collect();
        let (o, x) = (Head::Circle, Head::Cross);
        assert_eq!(heads, [(o, o), (x, x), (o, o), (x, x)]);
        assert_eq!(why("flowchart LR\nA o-- B"), "edge text without a closing arrow");
    }

    #[test]
    fn dotted_edge_text_may_start_with_o_or_x() {
        let g = ok("flowchart LR\nA -.optional.-> B\nC-.x.->D\nE -.xhr.- F\nG -. o .-> H");
        assert_eq!(edges(&g), [("A", "B"), ("C", "D"), ("E", "F"), ("G", "H")]);
        let labels: Vec<_> = g.edges.iter().map(|e| e.label.as_deref().unwrap()).collect();
        assert_eq!(labels, ["optional", "x", "xhr", "o"]);
        assert!(g.edges.iter().all(|e| e.stroke == Stroke::Dotted));
        assert_eq!(g.edges[2].head_to, Head::None);
        // After `--` and `==`, `o` and `x` are still heads.
        let g = ok("flowchart LR\nA --oB\nC ==xD");
        assert_eq!(ids(&g), ["A", "B", "C", "D"]);
    }

    #[test]
    fn invisible_links_may_be_longer() {
        let g = ok("flowchart LR\nA ~~~~ B\nC ~~~ D\nE ~~~oF");
        assert_eq!(edges(&g), [("A", "B"), ("C", "D"), ("E", "oF")]);
        let lens: Vec<_> = g.edges.iter().map(|e| e.min_len).collect();
        assert_eq!(lens, [2, 1, 1]);
        assert!(g.edges.iter().all(|e| e.stroke == Stroke::Invisible && e.head_to == Head::None));
    }

    #[test]
    fn link_length_is_the_minimum_rank_distance() {
        let g = ok("flowchart TD
            A --> B
            B ---> C
            C ----> D
            D --- E
            E ---- F
            F -.-> G
            G -..-> H
            H ==> I
            I ===> J
            J ~~~ K");
        let lens: Vec<_> = g.edges.iter().map(|e| e.min_len).collect();
        assert_eq!(lens, [1, 2, 3, 1, 2, 1, 2, 1, 2, 1]);
        assert_eq!(g.edges[9].stroke, Stroke::Invisible);
    }

    #[test]
    fn groups_make_every_edge() {
        let g = ok("flowchart TD\nA & B --> C & D --> E");
        assert_eq!(edges(&g), [("A", "C"), ("A", "D"), ("B", "C"), ("B", "D"), ("C", "E"), ("D", "E")]);
    }

    #[test]
    fn shapes() {
        let g = ok(r"flowchart TD
            A([s]) & B[[s]] & C[(s)] & D((s)) & E(((s))) & F>s] & G{s}
            H{{s}} & I[/s/] & J[\s\] & K[/s\] & L[\s/] & M(s) & N[s]");
        use Shape::*;
        let shapes: Vec<_> = g.nodes.iter().map(|n| n.shape).collect();
        let want = [Stadium, Subroutine, Cylinder, Circle, DCircle, Asym, Decision];
        assert_eq!(shapes[..7], want);
        assert_eq!(shapes[7..], [Hexagon, Slant, Slant, Slant, Slant, Round, Rect]);
        let slants: Vec<_> = g.nodes[8..12].iter().map(|n| n.slant.unwrap()).collect();
        assert_eq!(slants, [('/', '/'), ('\\', '\\'), ('/', '\\'), ('\\', '/')]);
        assert_eq!(g.nodes[5].slant, Some(('>', '│')));
        assert!(g.simplified.is_empty());
    }

    #[test]
    fn shapes_by_name() {
        let g = ok("flowchart TD
            A@{ shape: cyl, label: \"Orders DB\" } --> B@{ shape: bolt }
            C@{ shape: lean-l }
            D@{ shape: text, label: \"Just text\" }
            e1@{ animate: true }
            A e2@--> D");
        assert_eq!((g.nodes[0].shape, g.nodes[0].label.as_str()), (Shape::Cylinder, "Orders DB"));
        assert_eq!(g.nodes[1].shape, Shape::Rect);
        assert!(g.simplified.contains("shape"));
        assert_eq!((g.nodes[2].shape, g.nodes[2].slant), (Shape::Slant, Some(('\\', '\\'))));
        assert_eq!(g.nodes[3].shape, Shape::Bare);
        assert_eq!(edges(&g), [("A", "B"), ("A", "D")]);
        assert_eq!(why("flowchart TD\nsubgraph S\nA\nend\nS@{ view: collapsed }"), "collapsed subgraphs");
    }

    #[test]
    fn shape_data_may_be_single_quoted() {
        let g = ok("flowchart LR
            A@{ shape: 'cyl', label: 'Orders DB' } --> B
            C@{ shape: cyl, label: 'Orders, Inc' }
            D@{ label: 'It''s \"here\"' }
            E@{ label: \"a}b\" }");
        assert_eq!((g.nodes[0].shape, g.nodes[0].label.as_str()), (Shape::Cylinder, "Orders DB"));
        assert!(g.simplified.is_empty());
        let labels: Vec<_> = g.nodes[2..].iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, ["Orders, Inc", "It's \"here\"", "a}b"]);
        assert_eq!(why("flowchart LR\nA --> B\nB@{ view: 'collapsed' }"), "collapsed subgraphs");
    }

    #[test]
    fn shape_data_over_several_lines() {
        let g = ok("flowchart TD\nA@{\n  shape: cyl\n  label: \"Orders DB\"\n}\nA --> B");
        assert_eq!((g.nodes[0].shape, g.nodes[0].label.as_str()), (Shape::Cylinder, "Orders DB"));
        assert_eq!(edges(&g), [("A", "B")]);
        // A quoted value may go on over lines; YAML joins them with a space.
        let g = ok("flowchart TD\nA@{ shape: cyl\n  label: 'Orders\n  DB' } --> B; C");
        assert_eq!((g.nodes[0].shape, g.nodes[0].label.as_str()), (Shape::Cylinder, "Orders DB"));
        assert_eq!(ids(&g), ["A", "B", "C"]);
        // YAML comments; a key inside one is not read.
        let g = ok("flowchart TD\nA@{\n  # the store\n  shape: cyl # a database\n  label: \"Orders # 1\" # shown\n  # label: old\n}");
        assert_eq!((g.nodes[0].shape, g.nodes[0].label.as_str()), (Shape::Cylinder, "Orders # 1"));
        // Edge settings over several lines are skipped too.
        let g = ok("flowchart LR\nA e1@--> B\ne1@{\n  animate: true\n}");
        assert_eq!(ids(&g), ["A", "B"]);
        // A `}` far below can't swallow the statements in between.
        assert_eq!(why("flowchart TD\nA@{ shape: rect\nB --> C\nD{x}"), "unexpected text \"B --> C D{x\" in @{ }");
        assert_eq!(why("flowchart TD\nA@{ shape: rect\nB --> C"), "unterminated @{");
    }

    #[test]
    fn metadata_skips_only_edge_settings() {
        // Label text never decides: these are nodes and edges.
        let g = ok("flowchart LR
            x --> y
            start@{ shape: stadium, label: \"Load animation\" } --> play
            fit@{ shape: rect, label: \"fit curve\" } --> plot
            v@{ label: \"view: collapsed\" } --> w
            y@{ shape: stadium, label: \"Run animation\" }");
        assert_eq!(edges(&g), [("x", "y"), ("start", "play"), ("fit", "plot"), ("v", "w")]);
        let labels: Vec<_> = g.nodes.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, ["x", "Run animation", "Load animation", "play", "fit curve", "plot", "view: collapsed", "w"]);
        // Edge settings, by key or by a known edge id, are skipped.
        let g = ok("flowchart LR\nA e1@--> B\ne1@{ animation: fast }\ne1@{ curve: linear }\ne1@{ label: \"x\" }\nB");
        assert_eq!(ids(&g), ["A", "B"]);
    }

    #[test]
    fn group_links_have_a_budget() {
        // 70 × 70 edges, each with a 1,000-byte label.
        let side: Vec<String> = (0..70).map(|i| format!("n{i}")).collect();
        let side = side.join(" & ");
        let label = "x".repeat(1000);
        let st = format!("{side} -->|{label}| {side}");
        assert_eq!(parse("flowchart TD", &[st]), Err(NotDrawn::TooLarge));
        // The same group with a short label is fine.
        let g = parse("flowchart TD", &[format!("{side} -->|ok| {side}")]).unwrap();
        assert_eq!(g.edges.len(), 4900);
        // Many labelled edges one by one are fine too.
        let many: Vec<String> = (0..300).map(|i| format!("a{i} -->|{}| b{i}", "y".repeat(100))).collect();
        assert_eq!(parse("flowchart TD", &many).unwrap().edges.len(), 300);
    }

    #[test]
    fn labels_are_decoded() {
        let g = ok("flowchart TD\nA[\"Hello<br>world #quot;x#quot;\"] -->|\"`**big** move`\"| B[\"a;b\"]");
        assert_eq!(g.nodes[0].label, "Hello\nworld \"x\"");
        assert_eq!(g.edges[0].label.as_deref(), Some("big move"));
        assert_eq!(g.nodes[1].label, "a;b");
    }

    #[test]
    fn markdown_strings_over_several_lines() {
        // The examples from Mermaid's documentation.
        let g = ok("flowchart LR
  a(\"`The **cat**
  in the hat`\") -- \"edge label\" --> b{{\"`The **dog** in the hog`\"}}");
        assert_eq!(g.nodes[0].label, "The cat\nin the hat");
        assert_eq!(g.nodes[1].label, "The dog in the hog");
        assert_eq!(g.edges[0].label.as_deref(), Some("edge label"));
        let g = ok("flowchart LR\nnewLines[\"`Line1\nLine 2\nLine 3`\"] --> x");
        assert_eq!(g.nodes[0].label, "Line1\nLine 2\nLine 3");
    }

    #[test]
    fn semicolons_inside_edge_text_and_shapes() {
        let g = ok("flowchart LR\nA -.->|retry; backoff| B\nC>Flag; raised] --> D\nE -- a;b --> F; G --> H");
        let labels: Vec<_> = g.edges.iter().map(|e| e.label.as_deref()).collect();
        assert_eq!(labels, [Some("retry; backoff"), None, Some("a;b"), None]);
        assert_eq!(g.nodes[g.ids["C"]].label, "Flag; raised");
        let g = ok("graph TD; A-->B&C; C-->D;");
        assert_eq!(edges(&g), [("A", "B"), ("A", "C"), ("C", "D")]);
    }

    #[test]
    fn cjk_and_emoji_ids_and_labels() {
        let g = ok("flowchart LR\n开始 --> 结束\nA[認証サービス] --> B[(データベース)]\n😀 --> é.ü-ß");
        assert_eq!(ids(&g), ["开始", "结束", "A", "B", "😀", "é.ü-ß"]);
        assert_eq!(g.nodes[2].label, "認証サービス");
        // Ideographic punctuation ends an id.
        assert_eq!(why("flowchart LR\nA --> B。"), "unexpected text \"。\"");
    }

    #[test]
    fn subgraphs() {
        let g = ok("flowchart TB
            c1 --> a2
            subgraph one [Group one]
              a1 --> a2
              subgraph inner
                i1
              end
            end
            subgraph \"Two words\"
              b1
            end
            subgraph three words here
              direction LR
              c1
            end");
        let titles: Vec<_> = g.clusters.iter().map(|c| (c.id.as_str(), c.title.as_str(), c.parent)).collect();
        assert_eq!(
            titles,
            [
                ("one", "Group one", None),
                ("inner", "inner", Some(0)),
                ("Two words", "Two words", None),
                ("three words here", "three words here", None)
            ]
        );
        // c1 and a2 were named outside any subgraph first, but a subgraph
        // still takes them, as in Mermaid.
        let members: Vec<_> = g.clusters.iter().map(|c| c.members.clone()).collect();
        assert_eq!(members, [vec![2, 1], vec![3], vec![4], vec![0]]);
        assert_eq!(g.nodes[2].cluster, Some(0));
        assert_eq!(g.nodes[0].cluster, Some(3));
        assert!(g.simplified.contains("subgraph") && g.simplified.contains("subgraph direction"));
        assert_eq!(why("flowchart TB\nsubgraph A\nx"), "subgraph without \"end\"");
        assert_eq!(why("flowchart TB\nsubgraph\nend"), "subgraph without a name");
    }

    /// Each subgraph's members, by node id.
    fn members(g: &FlowGraph) -> Vec<(&str, Vec<&str>)> {
        let id = |k: usize| g.nodes[k].id.as_str();
        g.clusters.iter().map(|c| (c.id.as_str(), c.members.iter().map(|&k| id(k)).collect())).collect()
    }

    #[test]
    fn a_subgraph_takes_the_nodes_its_body_names() {
        // The example from Mermaid's documentation.
        let g = ok("flowchart TB
            c1-->a2
            subgraph one
            a1-->a2
            end
            subgraph two
            b1-->b2
            end
            subgraph three
            c1-->c2
            end");
        assert_eq!(members(&g), [("one", vec!["a1", "a2"]), ("two", vec!["b1", "b2"]), ("three", vec!["c1", "c2"])]);
        // Edges first, then the groups.
        let g = ok("flowchart LR\nA --> B --> C\nsubgraph G1 [Group one]\nA\nB\nend");
        assert_eq!(members(&g), [("G1", vec!["A", "B"])]);
        assert_eq!(g.nodes[g.ids["C"]].cluster, None);
        // An inner subgraph closes first and wins.
        let g = ok("flowchart LR\nsubgraph outer\nx --> y\nsubgraph inner\ny\nend\nend");
        assert_eq!(members(&g), [("outer", vec!["x"]), ("inner", vec!["y"])]);
        assert_eq!(g.nodes[g.ids["y"]].cluster, Some(1));
        // A subgraph closed earlier keeps its node, and naming a node at the
        // top level later changes nothing.
        let g = ok("flowchart LR\nsubgraph S\na\nend\nsubgraph T\na --> b\nend\na --> c");
        assert_eq!(members(&g), [("S", vec!["a"]), ("T", vec!["b"])]);
        // A reopened subgraph keeps what it took the first time.
        let g = ok("flowchart LR\nsubgraph S\na\nend\nsubgraph S\nb --> a\nend");
        assert_eq!(members(&g), [("S", vec!["a", "b"])]);
    }

    #[test]
    fn an_edge_to_a_subgraph_moves_to_its_first_member() {
        let g = ok("flowchart LR\nsubgraph S\n  a --> b\nend\nx --> S\nS --> y");
        assert_eq!(edges(&g), [("a", "b"), ("x", "a"), ("a", "y")]);
        assert!(g.nodes[g.ids["S"]].dead);
        assert!(g.simplified.contains("subgraph edge"));
        assert_eq!(why("flowchart LR\nsubgraph S\nend\nx --> S"), "edge to an empty subgraph");
        // A subgraph id named inside another subgraph that is an edge end:
        // the edge never ends at the hidden node.
        let g = ok("flowchart TD\nsubgraph X\n  a\nend\nsubgraph Y\n  X\n  n\nend\nt --> Y");
        assert_eq!(edges(&g), [("t", "n")]);
        // The member was named before the subgraph took it.
        let g = ok("flowchart LR\nA --> B\nsubgraph G\nA\nend\nC --> G");
        assert_eq!(edges(&g), [("A", "B"), ("C", "A")]);
    }

    #[test]
    fn an_edge_to_a_subgraph_of_subgraphs_moves_into_them() {
        let g = ok("flowchart LR\nuser --> Cloud\nsubgraph Cloud\nsubgraph VPC\napi --> db\nend\nend");
        assert_eq!(edges(&g), [("user", "api"), ("api", "db")]);
        let g = ok("flowchart LR\nsubgraph Cloud\nsubgraph VPC\napi --> db\nend\nend\nCloud --> user");
        assert_eq!(edges(&g), [("api", "db"), ("api", "user")]);
        // Its own members come before those of nested subgraphs.
        let g = ok("flowchart LR\nsubgraph A\nsubgraph B\nb\nend\na\nend\nx --> A");
        assert_eq!(edges(&g), [("x", "a")]);
        assert_eq!(why("flowchart LR\nsubgraph A\nsubgraph B\nend\nend\nx --> A"), "edge to an empty subgraph");
    }

    #[test]
    fn an_edge_moved_off_a_subgraph_keeps_its_name() {
        let g = ok("flowchart LR\nsubgraph S\n  a --> b\nend\nx --> S\nS --> y\nS --> S");
        let ends: Vec<_> = g.edges.iter().map(|e| (e.from_cluster, e.to_cluster)).collect();
        assert_eq!(ends, [(None, None), (None, Some(0)), (Some(0), None), (Some(0), Some(0))]);
        let g = ok("flowchart LR\nuser --> Cloud\nsubgraph Cloud\nsubgraph VPC\napi\nend\nend");
        assert_eq!((g.edges[0].from_cluster, g.edges[0].to_cluster), (None, Some(0)));
    }

    /// Each subgraph's id and parent's id.
    fn tree(g: &FlowGraph) -> Vec<(&str, Option<&str>)> {
        g.clusters.iter().map(|c| (c.id.as_str(), c.parent.map(|p| g.clusters[p].id.as_str()))).collect()
    }

    #[test]
    fn a_subgraph_named_in_another_body_is_nested_there() {
        // As in Mermaid, naming a subgraph's id nests it like writing it
        // there. The parent comes first, so outlines indent it under it.
        let g = ok("flowchart LR\nsubgraph Inner\napi --> db\nend\nsubgraph Outer\nInner\nweb\nend");
        assert_eq!(tree(&g), [("Outer", None), ("Inner", Some("Outer"))]);
        assert_eq!(members(&g), [("Outer", vec!["Inner", "web"]), ("Inner", vec!["api", "db"])]);
        assert!(g.nodes[g.ids["Inner"]].dead);
        assert_eq!(g.nodes[g.ids["api"]].cluster, Some(1));
        // An edge to the outer subgraph reaches the nested one's members.
        let g = ok("flowchart LR\nsubgraph Inner\napi --> db\nend\nsubgraph Outer\nInner\nend\nuser --> Outer");
        assert_eq!(edges(&g), [("api", "db"), ("user", "api")]);
        assert_eq!(g.edges[1].to_cluster, Some(0));
        // Named before the subgraph is declared.
        let g = ok("flowchart LR\nsubgraph Outer\nInner\nend\nsubgraph Inner\napi\nend\nuser --> Outer");
        assert_eq!(tree(&g), [("Outer", None), ("Inner", Some("Outer"))]);
        assert_eq!(edges(&g), [("user", "api")]);
        // The first subgraph to close wins, even over the one it is opened in.
        let g = ok("flowchart LR\nsubgraph P\nsubgraph Inner\na\nend\nsubgraph Outer\nInner\nb\nend\nend");
        assert_eq!(tree(&g), [("P", None), ("Outer", Some("P")), ("Inner", Some("Outer"))]);
        let g = ok("flowchart LR\nsubgraph X\nsubgraph Inner\na\nend\nend\nsubgraph Outer\nInner\nb\nend");
        assert_eq!(tree(&g), [("X", None), ("Inner", Some("X")), ("Outer", None)]);
        let g = ok("flowchart LR\nsubgraph Outer\nInner\nb\nend\nsubgraph X\nsubgraph Inner\na\nend\nend");
        assert_eq!(tree(&g), [("Outer", None), ("Inner", Some("Outer")), ("X", None)]);
    }

    #[test]
    fn subgraphs_that_name_each_other_make_no_cycle() {
        let g = ok("flowchart LR\nsubgraph A\nB\nx\nend\nsubgraph B\nA\ny\nend");
        assert_eq!(tree(&g), [("A", None), ("B", Some("A"))]);
        let g = ok("flowchart LR\nsubgraph A\nsubgraph B\nA\nx\nend\nend");
        assert_eq!(tree(&g), [("A", None), ("B", Some("A"))]);
        let g = ok("flowchart LR\nsubgraph A\nsubgraph B\nx\nend\nend\nsubgraph B\nA\nend");
        assert_eq!(tree(&g), [("A", None), ("B", Some("A"))]);
        assert_eq!(why("flowchart LR\nsubgraph A\nB\nend\nsubgraph B\nA\nend\nx --> A"), "edge to an empty subgraph");
        // Subgraphs that name each other in a ring, deeply nested.
        let mut src = String::from("flowchart LR\n");
        for i in 0..200 {
            src.push_str(&format!("subgraph s{i}\ns{}\nn{i}\n", (i + 1) % 200));
        }
        src.push_str(&"end\n".repeat(200));
        let g = ok(&src);
        let t = tree(&g);
        assert_eq!((t.len(), t[0], t[199]), (200, ("s0", None), ("s199", Some("s198"))));
    }

    #[test]
    fn deep_subgraphs_named_at_the_bottom_parse_quickly() {
        // Each name checks for a cycle by walking up the nesting.
        let n = 9_000;
        let mut st: Vec<String> = (0..n).map(|i| format!("subgraph s{i}")).collect();
        st.extend((0..n).map(|i| format!("s{i}")));
        st.extend((0..n).map(|_| "end".to_string()));
        let start = std::time::Instant::now();
        assert_eq!(parse("flowchart LR", &st), Err(NotDrawn::TooLarge));
        assert!(start.elapsed().as_secs() < 2);
    }

    #[test]
    fn subgraphs_are_listed_parent_first() {
        // A subgraph reopened inside one declared after another.
        let g = ok("flowchart LR\nsubgraph A\na\nend\nsubgraph B\nb\nend\nsubgraph A\nsubgraph C\nc\nend\nend");
        assert_eq!(tree(&g), [("A", None), ("C", Some("A")), ("B", None)]);
        assert_eq!(members(&g), [("A", vec!["a"]), ("C", vec!["c"]), ("B", vec!["b"])]);
        let clusters: Vec<_> = g.nodes.iter().map(|n| n.cluster).collect();
        assert_eq!(clusters, [Some(0), Some(2), Some(1)]);
    }

    #[test]
    fn a_title_over_several_lines_keeps_the_id() {
        let g = ok("flowchart LR\nsubgraph S [\"`Two\n lines`\"]\na\nend\nx --> S");
        assert_eq!((g.clusters[0].id.as_str(), g.clusters[0].title.as_str()), ("S", "Two\nlines"));
        assert_eq!(edges(&g), [("x", "a")]);
        assert_eq!(g.edges[0].to_cluster, Some(0));
    }

    #[test]
    fn reopened_subgraphs_cannot_nest_in_themselves() {
        let g = ok("flowchart TD\nsubgraph A\nsubgraph B\nsubgraph A\nx\nend\nend\nend");
        assert_eq!(g.clusters.len(), 2);
        assert_eq!(g.clusters[0].parent, None);
        assert_eq!(g.clusters[1].parent, Some(0));
    }

    #[test]
    fn syntax_errors_name_the_text() {
        assert_eq!(why("flowchart TD\nA --> B C"), "unexpected text \"C\"");
        assert_eq!(why("flowchart TD\nE[List[Result]]"), "unexpected text \"]\"");
        assert_eq!(why("flowchart TD\nA[\"x\" y]"), "text after a quoted label");
        assert_eq!(why("flowchart TD\nA[open"), "unterminated node shape");
        assert_eq!(why("flowchart TD\nA[\"open]"), "unterminated quote");
        assert_eq!(why("flowchart TD\nA@{ shape: rect"), "unterminated @{");
        assert_eq!(why("flowchart TD\n--> B"), "expected a node at \"--> B\"");
        let long = why(&format!("flowchart TD\nA --> B {}", "x ".repeat(50)));
        assert!(pre::width(&long) <= "unexpected text ".len() + 32, "{long}");
    }

    #[test]
    fn odd_input_never_panics() {
        let odd = [
            "A[", "A[\"", "A@{", "-->", "A -- B", "A-->", "&", "A & ", "A[\"x\" y]", "A-.->", "A -. x", "A -.x.- ",
            "A@{ shape: }", ":::", "A:::", "A:::b:::c", "A ---", "A x", "A o", "A <", "A <-", "A <--", "A <-- x",
            "e1@", "A e1@", "A e1@--> B", "A --|x", "A -->|x", "A -->|x| ", "é-", "a.", "a-.", "a--.", "😀--",
            "A -- \"", "A -- \"x", "A -- \"`x", "A -- \"`x`\"", "A -->|\"", "A -->|\"|", "A -->|é\"|", "A -. \"é\" .",
            "A@{ label: 'x", "A@{ label: '' }", "A@{\n", "A@{\n#", "A@{\n# x: \"y\n}", "A@{ x: \"}\" ", "A[/é", "A[\\é\\",
            "v", "^", "<", ">", "A[/x/] & A[\\x", "A@{\n  shape: é # é\n}",
            "A[\"\"]", "A[ ]", "A(())", "A((()))", "A[/x", "A[\\x", ">x]", "A>", "A{", "A{{x}", "subgraph [x]",
            "\u{fffd}", "A\t-->\tB", "A -->\u{3000}B", "A --o", "A--x", "A ~~~", "A ~~~ ~~~ B", "A ==", "A ==>",
            "", " ", "end", "subgraph", "direction", "direction LR", "title", "title -->", "A o--", "A x-- t",
            "A -.o", "A ~~~~", "~~~", "e1@{", "e1@{}", "e1@{ animate }", "A@{ view: collapsed } --> B", "A -.-o",
        ];
        for s in odd {
            let _ = parse("flowchart TD", &[s.to_string()]);
            let _ = parse(&format!("graph LR {s}"), &[]);
        }
    }

    #[test]
    fn long_groups_of_slanted_shapes_parse_quickly() {
        // Each shape has two closers; looking for the absent one to the end
        // of the line for every node took seconds.
        for unit in ["A[/x/]&", "A[\\x\\]&", "A[/x\\]&", "A[\\x/]&"] {
            let st = format!("{}A", unit.repeat(36_000));
            let start = std::time::Instant::now();
            let g = parse("flowchart LR", &[st]).unwrap();
            assert_eq!(g.nodes.len(), 1);
            assert!(start.elapsed().as_secs() < 2, "{unit}");
        }
    }

    #[test]
    fn huge_groups_are_too_large() {
        let side: Vec<String> = (0..200).map(|i| format!("n{i}")).collect();
        let side = side.join(" & ");
        assert_eq!(parse("flowchart TD", &[format!("{side} --> {side}")]), Err(NotDrawn::TooLarge));
    }
}
