//! Outlines and transcripts (plan section 6.5): what a reader sees when no
//! drawing fits. They always fit the width, and a long one stops after
//! `MAX_ROWS` rows with a line that says how many lines are left out.

use super::flow::{FlowGraph, Head, Stroke};
use super::pre::{cut, width, wrap};
use super::seq::{Event, PartKind, SeqDiagram, SeqHead};
use super::{Drawing, Kind, LabelRef, Role};

/// Rows an outline or transcript may have. The lines that don't fit are
/// counted in one last line instead.
const MAX_ROWS: usize = 2000;

/// The flowchart as text: subgraphs first, then one line per edge, then
/// nodes with no visible edges. An edge to or from a subgraph names the
/// subgraph.
pub fn flow_outline(g: &FlowGraph, width: usize) -> Drawing {
    let mut out = Out::new(width);
    let name = |k: usize| {
        let n = &g.nodes[k];
        if n.label.is_empty() { n.id.clone() } else { n.label.replace('\n', " ") }
    };
    let title = |c: usize| {
        let cl = &g.clusters[c];
        if cl.title.trim().is_empty() { cl.id.clone() } else { cl.title.replace('\n', " ") }
    };
    // An edge end that named a subgraph shows the subgraph, not the member
    // the edge was attached to.
    let end = |k: usize, cluster: Option<usize>| match cluster {
        Some(c) if c < g.clusters.len() => title(c),
        _ => name(k),
    };
    for (c, cl) in g.clusters.iter().enumerate() {
        if out.skip() {
            continue;
        }
        let members: Vec<String> = cl.members.iter().filter(|&&m| !g.nodes[m].dead).map(|&m| name(m)).collect();
        let line = if members.is_empty() { title(c) } else { format!("{}: {}", title(c), members.join(", ")) };
        out.line(depth(g, c), &[(Role::NodeText, line)]);
    }
    let mut linked = vec![false; g.nodes.len()];
    for e in g.edges.iter().filter(|e| e.stroke != Stroke::Invisible) {
        if e.from_cluster.is_none() {
            linked[e.from] = true;
        }
        if e.to_cluster.is_none() {
            linked[e.to] = true;
        }
        if out.skip() {
            continue;
        }
        let line = match e.stroke {
            Stroke::Dotted => "┄",
            Stroke::Thick => "━",
            _ => "─",
        };
        let left = match e.head_from {
            Head::Arrow => "◂",
            Head::Circle => "o",
            Head::Cross => "✕",
            Head::None => "",
        };
        let right = match e.head_to {
            Head::Arrow => "▸",
            Head::Circle => "o",
            Head::Cross => "✕",
            Head::None => line,
        };
        let from = end(e.from, e.from_cluster);
        let mut runs = vec![(Role::NodeText, format!("{from} ")), (Role::Edge, format!("{left}{line}"))];
        if let Some(label) = &e.label {
            runs.push((Role::EdgeLabel, label.replace('\n', " ")));
            runs.push((Role::Edge, line.to_string()));
        }
        runs.push((Role::Edge, right.to_string()));
        runs.push((Role::NodeText, format!(" {}", end(e.to, e.to_cluster))));
        out.line(0, &runs);
    }
    for (k, n) in g.nodes.iter().enumerate() {
        if !linked[k] && !n.dead && !out.skip() {
            out.line(0, &[(Role::NodeText, name(k))]);
        }
    }
    out.finish(Kind::Flowchart)
}

/// How deeply subgraph `c` is nested.
fn depth(g: &FlowGraph, c: usize) -> usize {
    let mut d = 0;
    let mut up = g.clusters[c].parent;
    while let Some(p) = up
        && d < g.clusters.len()
    {
        d += 1;
        up = g.clusters[p].parent;
    }
    d
}

/// The sequence diagram as a numbered transcript. Block contents are
/// indented under their header.
///
/// When some participant has no line of its own (no message, note, create,
/// destroy or box), the first line lists every participant, so none is
/// lost. Each `box` then gets a line with its title and members.
///
/// Without `autonumber`, messages are numbered 1, 2, 3. With it, they are
/// numbered as Mermaid numbers them: each `autonumber` statement takes
/// effect where it stands, the count runs on through messages that show no
/// number, and messages before the first one or after `autonumber off` get
/// no number. `create` and `destroy` get a line where they happen;
/// activations don't.
pub fn seq_transcript(sd: &SeqDiagram, width: usize) -> Drawing {
    let mut out = Out::new(width);
    let name = |k: usize| sd.participants[k].label.replace('\n', " ");
    let mut named = vec![false; sd.participants.len()];
    for ev in &sd.events {
        match ev {
            Event::Message { from, to, .. } => {
                named[*from] = true;
                named[*to] = true;
            }
            Event::Note { who, .. } => who.iter().for_each(|&k| named[k] = true),
            Event::Destroy(k) => named[*k] = true,
            _ => {}
        }
    }
    // `create` lines, in order, by the number of events before them.
    let mut creates: Vec<(usize, usize)> =
        sd.participants.iter().enumerate().filter_map(|(k, p)| p.created.map(|at| (at, k))).collect();
    creates.sort_unstable();
    for &(_, k) in &creates {
        named[k] = true;
    }
    for b in &sd.boxes {
        b.members.iter().for_each(|&k| named[k] = true);
    }
    if named.contains(&false) {
        let all: Vec<String> = (0..sd.participants.len())
            .map(|k| if sd.participants[k].kind == PartKind::Actor { format!("{} (actor)", name(k)) } else { name(k) })
            .collect();
        out.line(0, &[(Role::NodeText, format!("Participants: {}", all.join(", ")))]);
    }
    for b in &sd.boxes {
        let title = b.title.replace('\n', " ");
        if (title.is_empty() && b.members.is_empty()) || out.skip() {
            continue;
        }
        let members: Vec<String> = b.members.iter().map(|&k| name(k)).collect();
        let line = match (title.is_empty(), members.is_empty()) {
            (_, true) => format!("box {title}"),
            (true, false) => format!("box: {}", members.join(", ")),
            (false, false) => format!("box {title}: {}", members.join(", ")),
        };
        out.line(0, &[(Role::NodeText, line)]);
    }
    let mut num: u64 = 1;
    let mut step: u64 = 1;
    let mut shown = sd.autonumbers.is_empty();
    let mut next_auto = 0;
    let mut level = 0usize;
    let mut next_create = 0;
    for (i, ev) in sd.events.iter().enumerate() {
        while let Some(&(at, k)) = creates.get(next_create)
            && at <= i
        {
            next_create += 1;
            create_line(&mut out, level, sd, k);
        }
        while let Some(&(at, a)) = sd.autonumbers.get(next_auto)
            && at <= i
        {
            next_auto += 1;
            // Mermaid treats a 0 as left out.
            num = a.start.filter(|&n| n != 0).unwrap_or(num);
            step = a.step.filter(|&n| n != 0).unwrap_or(step);
            shown = a.visible;
        }
        // Text is only built for lines that are shown.
        match ev {
            Event::Message { from, to, stroke, head, both, label, .. } => {
                if !out.skip() {
                    let line = if *stroke == Stroke::Dotted { "┄" } else { "─" };
                    let left = if *both { "◂" } else { "" };
                    let right = match head {
                        SeqHead::None => line,
                        SeqHead::Arrow => "▸",
                        SeqHead::Cross => "✕",
                        SeqHead::Async => "▹",
                    };
                    let number = if shown { format!("{num}. ") } else { String::new() };
                    let runs = [
                        (Role::NodeText, format!("{number}{} ", name(*from))),
                        (Role::Edge, format!("{left}{line}{right}")),
                        (Role::NodeText, format!(" {}: {}", name(*to), label.replace('\n', " "))),
                    ];
                    out.line(level, &runs);
                }
                num = num.saturating_add(step);
            }
            Event::Note { at, who, label } => {
                if !out.skip() {
                    let who: Vec<String> = who.iter().map(|&k| name(k)).collect();
                    let text = format!("Note {} {}: {}", at.name(), who.join(", "), label.replace('\n', " "));
                    out.line(level, &[(Role::NodeText, text)]);
                }
            }
            Event::Open { kind, label } => {
                if !out.skip() {
                    out.line(level, &[(Role::NodeText, format!("{} {}", kind.name(), label.replace('\n', " ")))]);
                }
                level += 1;
            }
            Event::Divider { kind, label } => {
                if !out.skip() {
                    let text = format!("{} {}", kind.name(), label.replace('\n', " "));
                    out.line(level.saturating_sub(1), &[(Role::NodeText, text)]);
                }
            }
            Event::Close => {
                level = level.saturating_sub(1);
                if !out.skip() {
                    out.line(level, &[(Role::NodeText, "end".to_string())]);
                }
            }
            Event::Destroy(k) => {
                if !out.skip() {
                    out.line(level, &[(Role::NodeText, format!("destroy {}", name(*k)))]);
                }
            }
            Event::Activate(_) | Event::Deactivate(_) => {}
        }
    }
    for &(_, k) in &creates[next_create..] {
        create_line(&mut out, level, sd, k);
    }
    let mut d = out.finish(Kind::Sequence);
    d.title = sd.title.clone();
    d
}

/// The `create participant …` or `create actor …` line for participant `k`.
fn create_line(out: &mut Out, level: usize, sd: &SeqDiagram, k: usize) {
    if !out.skip() {
        let p = &sd.participants[k];
        out.line(level, &[(Role::NodeText, format!("create {} {}", p.kind.name(), p.label.replace('\n', " ")))]);
    }
}

/// Rows of an outline or transcript, and a label for each line so search
/// finds them.
struct Out {
    width: usize,
    rows: Vec<Vec<(Role, String)>>,
    labels: Vec<LabelRef>,
    /// Lines left out because the rows reached [`MAX_ROWS`].
    left_out: usize,
}

impl Out {
    fn new(width: usize) -> Out {
        Out { width: width.max(1), rows: Vec::new(), labels: Vec::new(), left_out: 0 }
    }

    /// Whether the outline is full, so the caller should leave its next
    /// line out. That line is counted.
    fn skip(&mut self) -> bool {
        if self.left_out > 0 {
            self.left_out += 1;
        }
        self.left_out > 0
    }

    /// Add one line, unless it would take the rows past [`MAX_ROWS`]: then
    /// it and every later line are left out and counted.
    fn line(&mut self, level: usize, runs: &[(Role, String)]) {
        if self.left_out > 0 {
            self.left_out += 1;
            return;
        }
        let (rows, label) = self.wrap_line(level, runs);
        if self.rows.len() + rows.len() > MAX_ROWS {
            self.left_out = 1;
            return;
        }
        self.push(rows, label);
    }

    fn push(&mut self, rows: Vec<Vec<(Role, String)>>, label: LabelRef) {
        self.rows.extend(rows);
        if !label.pieces.is_empty() {
            self.labels.push(label);
        }
    }

    /// One line as rows, indented 2 columns per `level` and wrapped with a
    /// 4-column hanging indent, and its label for rows numbered from the
    /// current end. A line starts at most a third of the width in, and its
    /// later rows at most 2 columns past that, so every row fits. From 5
    /// columns up, a later row starts deeper than the next line can (at
    /// most one level deeper), so it never reads as a line of its own.
    fn wrap_line(&self, level: usize, runs: &[(Role, String)]) -> (Vec<Vec<(Role, String)>>, LabelRef) {
        let cap = self.width / 3;
        let first = (2 * level).min(cap);
        let hang = (first + 4).min(cap + 2).min(self.width - 1);
        // One role per character, so the wrapped text gets its roles back.
        let chars: Vec<(char, Role)> = runs.iter().flat_map(|(r, s)| s.chars().map(move |c| (c, *r))).collect();
        let text_from = |pos: usize| chars[pos..].iter().map(|(c, _)| *c).collect::<String>();
        // The first row, then the rest of the line wrapped at the hanging indent.
        let mut pos = skip_space(&chars, 0);
        let head = wrap(&text_from(pos), self.width - first).into_iter().next().unwrap_or_default();
        let mut rows = vec![(first, take(&chars, &mut pos, &head))];
        pos = skip_space(&chars, pos);
        if pos < chars.len() {
            for piece in wrap(&text_from(pos), self.width - hang) {
                pos = skip_space(&chars, pos);
                rows.push((hang, take(&chars, &mut pos, &piece)));
            }
        }
        let mut label = LabelRef::default();
        let mut out = Vec::new();
        for (indent, mut row) in rows {
            let text: String = row.iter().map(|(_, s)| s.as_str()).collect();
            let room = self.width - indent;
            if width(&text) > room {
                // Only a character wider than the room gets here.
                let role = row.first().map_or(Role::NodeText, |(r, _)| *r);
                row = vec![(role, cut(&text, room))];
            }
            let text: String = row.iter().map(|(_, s)| s.as_str()).collect();
            if text.is_empty() {
                continue;
            }
            label.pieces.push((self.rows.len() + out.len(), indent, text));
            match row.first_mut() {
                Some((Role::NodeText, s)) => s.insert_str(0, &" ".repeat(indent)),
                _ if indent > 0 => row.insert(0, (Role::NodeText, " ".repeat(indent))),
                _ => {}
            }
            out.push(row);
        }
        (out, label)
    }

    /// The drawing, ending with a line that counts the lines left out.
    fn finish(mut self, kind: Kind) -> Drawing {
        if self.left_out > 0 {
            let n = self.left_out;
            let more = if self.rows.is_empty() { "" } else { " more" };
            let text = format!("{n}{more} {} not shown", if n == 1 { "line" } else { "lines" });
            let (rows, label) = self.wrap_line(0, &[(Role::NodeText, text)]);
            self.push(rows, label);
        }
        let width = self.rows.iter().map(|r| r.iter().map(|(_, s)| width(s)).sum::<usize>()).max().unwrap_or(0);
        Drawing { kind, title: None, step: None, rows: self.rows, width, labels: self.labels, notes: Vec::new() }
    }
}

/// The index of the first character at or after `pos` that is not a space.
fn skip_space(chars: &[(char, Role)], mut pos: usize) -> usize {
    while chars.get(pos).is_some_and(|(c, _)| c.is_whitespace()) {
        pos += 1;
    }
    pos
}

/// The runs of one wrapped row, `text`, taken from `chars` at `pos`. A
/// space in `text` stands for a run of spaces in `chars`.
fn take(chars: &[(char, Role)], pos: &mut usize, text: &str) -> Vec<(Role, String)> {
    let mut runs: Vec<(Role, String)> = Vec::new();
    for c in text.chars() {
        let role = chars.get(*pos).map_or(Role::NodeText, |&(_, r)| r);
        if c == ' ' && chars.get(*pos).is_some_and(|(o, _)| o.is_whitespace()) {
            *pos = skip_space(chars, *pos);
        } else {
            *pos += 1;
        }
        match runs.last_mut() {
            Some((r, s)) if *r == role => s.push(c),
            _ => runs.push((role, c.to_string())),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::{flow, pre, seq};

    fn graph(src: &str) -> FlowGraph {
        let (_, body) = pre::preamble(src).unwrap();
        let st = pre::statements(&body);
        flow::parse::parse(&st[0], &st[1..]).unwrap()
    }

    fn sequence(src: &str) -> SeqDiagram {
        let (_, body) = pre::preamble(src).unwrap();
        let st = pre::statements(&body);
        seq::parse::parse(&st[0], &st[1..]).unwrap()
    }

    fn text(d: &Drawing) -> Vec<String> {
        d.rows.iter().map(|r| r.iter().map(|(_, s)| s.as_str()).collect()).collect()
    }

    /// Every row fits and has no trailing space, the width is the widest
    /// row, and each label piece is the text at its row and column.
    fn check(d: &Drawing, w: usize) {
        let rows = text(d);
        assert!(!rows.is_empty(), "an outline always has a row");
        let widest = rows.iter().map(|r| width(r)).max().unwrap_or(0);
        assert_eq!(d.width, widest);
        for r in &rows {
            assert!(width(r) <= w, "{r:?} is wider than {w}");
            assert!(!r.ends_with(' ') && !r.is_empty(), "{r:?}");
        }
        let mut seen = vec![false; rows.len()];
        for l in &d.labels {
            for (row, col, piece) in &l.pieces {
                assert_eq!(&rows[*row][*col..], piece.as_str());
                seen[*row] = true;
            }
        }
        assert!(seen.iter().all(|&s| s), "every row is in a label");
    }

    const FLOW: &str = "flowchart LR
        subgraph one [Group one]
          a[Receive order] --> b{In stock?}
          subgraph inner [Inner group]
            c[Back-order]
          end
        end
        b -->|yes| d[Ship it]
        b -- no --> c
        c -.-> e[(Orders DB)]
        d ==> f([Done])
        f --- g
        g --o h
        h x--x i
        j <--> k
        l ~~~ m
        lonely[A node with no edges]";

    #[test]
    fn outline_lists_subgraphs_then_edges_then_lone_nodes() {
        let d = flow_outline(&graph(FLOW), 80);
        assert_eq!(
            text(&d),
            [
                "Group one: Receive order, In stock?",
                "  Inner group: Back-order",
                "Receive order ─▸ In stock?",
                "In stock? ─yes─▸ Ship it",
                "In stock? ─no─▸ Back-order",
                "Back-order ┄▸ Orders DB",
                "Ship it ━▸ Done",
                "Done ── g",
                "g ─o h",
                "h ✕─✕ i",
                "j ◂─▸ k",
                "l",
                "m",
                "A node with no edges",
            ]
        );
        assert_eq!(d.step, None);
        assert_eq!(d.labels.len(), 14);
        // Roles: names are node text, arrows are edges, labels are edge labels.
        assert_eq!(
            d.rows[3],
            [
                (Role::NodeText, "In stock? ".to_string()),
                (Role::Edge, "─".to_string()),
                (Role::EdgeLabel, "yes".to_string()),
                (Role::Edge, "─▸".to_string()),
                (Role::NodeText, " Ship it".to_string()),
            ]
        );
    }

    #[test]
    fn outline_lines_wrap_with_a_hanging_indent() {
        let g = graph("flowchart TD\nA[The first step of a long process] --> B[The second step]");
        assert_eq!(text(&flow_outline(&g, 20)), ["The first step of a", "    long process ─▸", "    The second step"]);
        let g = graph("flowchart TD\nA[Averyveryverylongwordindeed] --> B");
        assert_eq!(text(&flow_outline(&g, 12)), ["Averyveryver", "    ylongwor", "    dindeed", "    ─▸ B"]);
        // A line starts at most a third of the width in; its later rows go
        // at most 2 columns further.
        let g = graph("flowchart TD\nsubgraph a\nsubgraph b\nsubgraph c\nx[Some words here]\nend\nend\nend");
        let rows = ["a", "  b", "    c: Some", "      words", "      here", "Some words", "    here"];
        assert_eq!(text(&flow_outline(&g, 12)), rows);
    }

    /// The indent of each row.
    fn indents(d: &Drawing) -> Vec<usize> {
        text(d).iter().map(|r| r.len() - r.trim_start().len()).collect()
    }

    #[test]
    fn wrapped_rows_never_look_like_nested_lines() {
        let g = graph("flowchart TD
            subgraph one [Billing and invoicing services]
              a[Invoice generator]
              subgraph two [Payments]
                b[Card processor]
              end
            end
            a --> b");
        let d = flow_outline(&g, 40);
        assert_eq!(
            text(&d)[..3],
            ["Billing and invoicing services: Invoice", "    generator", "  Payments: Card processor"]
        );
        let sd = sequence("sequenceDiagram\nloop Retry until the payment provider answers\nA->>B: poll\nend");
        let rows = ["loop Retry until the payment", "    provider answers", "  1. A ─▸ B: poll", "end"];
        assert_eq!(text(&seq_transcript(&sd, 30)), rows);
        // At every width from 5 columns, a later row of a line starts deeper
        // than the line after it, which is at most one level deeper.
        let deep = (0..8).map(|i| format!("loop retry number {i} of the job\n")).collect::<String>()
            + "A->>B: a message with several words\n"
            + &"end\n".repeat(8);
        let sd = sequence(&format!("sequenceDiagram\n{deep}"));
        for w in 5..=120 {
            let d = seq_transcript(&sd, w);
            let starts: Vec<usize> = d.labels.iter().map(|l| l.pieces[0].0).collect();
            let ind = indents(&d);
            for (n, &row) in starts.iter().enumerate() {
                let end = starts.get(n + 1).copied().unwrap_or(ind.len());
                if let Some(&next) = ind.get(end) {
                    for r in row + 1..end {
                        assert!(ind[r] > next, "width {w}, row {r}: {:?}", text(&d));
                        assert!(ind[r] > ind[row], "width {w}, row {r}: {:?}", text(&d));
                    }
                }
            }
        }
    }

    #[test]
    fn transcript_numbers_messages_and_indents_blocks() {
        let sd = sequence("sequenceDiagram
            participant C as Client
            participant API
            C->>API: GET /orders
            Note right of API: checks the token
            alt valid
              API-->>C: 200 OK
            else invalid
              API--xC: 401
            end
            loop
              C<<->>API: ping
            end
            activate API");
        let d = seq_transcript(&sd, 80);
        assert_eq!(
            text(&d),
            [
                "1. Client ─▸ API: GET /orders",
                "Note right of API: checks the token",
                "alt valid",
                "  2. API ┄▸ Client: 200 OK",
                "else invalid",
                "  3. API ┄✕ Client: 401",
                "end",
                "loop",
                "  4. Client ◂─▸ API: ping",
                "end",
            ]
        );
        assert_eq!(text(&seq_transcript(&sd, 20))[..3], ["1. Client ─▸ API:", "    GET /orders", "Note right of API:"]);
    }

    #[test]
    fn transcript_lists_participants_when_some_have_no_lines() {
        let only = ["participant A\nparticipant B", "participant A\nparticipant B\nactivate A"];
        for src in only {
            let d = seq_transcript(&sequence(&format!("sequenceDiagram\n{src}")), 80);
            assert_eq!(text(&d), ["Participants: A, B"]);
            assert_eq!(d.labels.len(), 1);
        }
        let d = seq_transcript(&sequence("sequenceDiagram\nactor Alice"), 80);
        assert_eq!(text(&d), ["Participants: Alice (actor)"]);
        let sd = sequence("sequenceDiagram
            participant API
            participant Unused as Never messaged
            actor Bob
            participant C as Client
            C->>API: GET /orders
            create participant D as Created one
            API->>D: hello
            D->>API: bye
            destroy API");
        assert_eq!(
            text(&seq_transcript(&sd, 80)),
            [
                "Participants: API, Never messaged, Bob (actor), Client, Created one",
                "1. Client ─▸ API: GET /orders",
                "create participant Created one",
                "2. API ─▸ Created one: hello",
                "3. Created one ─▸ API: bye",
                "destroy API",
            ]
        );
        // Everyone is named, so there is no list; a `create` at the end is kept.
        let sd = sequence("sequenceDiagram\nA->>B: x\nloop\ncreate actor C\nend\ncreate participant D");
        let rows = ["1. A ─▸ B: x", "loop", "  create actor C", "end", "create participant D"];
        assert_eq!(text(&seq_transcript(&sd, 80)), rows);
    }

    #[test]
    fn transcript_follows_autonumber() {
        let body = "C->>API: GET /orders\nAPI-->>C: 200 OK\nC-)API: async";
        let numbers = |head: &str| -> Vec<String> {
            let d = seq_transcript(&sequence(&format!("sequenceDiagram\n{head}\n{body}")), 80);
            text(&d).iter().map(|r| r.split('.').next().unwrap_or("").to_string()).collect()
        };
        assert_eq!(numbers("autonumber 10 5"), ["10", "15", "20"]);
        assert_eq!(numbers("autonumber 7"), ["7", "8", "9"]);
        assert_eq!(numbers("autonumber"), ["1", "2", "3"]);
        assert_eq!(numbers("%% none"), ["1", "2", "3"]);
        assert_eq!(numbers("autonumber 99999999999 4294967295"), ["99999999999", "104294967294", "108589934589"]);
        let max = u64::MAX.to_string();
        assert_eq!(numbers("autonumber 99999999999999999999999 5"), [max.as_str(), &max, &max]);
        // Mermaid reads a 0 as left out: the start stays 1, the step 1.
        assert_eq!(numbers("autonumber 5 0"), ["5", "6", "7"]);
        assert_eq!(numbers("autonumber 0"), ["1", "2", "3"]);
        assert_eq!(numbers("autonumber 0 0"), ["1", "2", "3"]);
    }

    #[test]
    fn autonumber_takes_effect_where_it_stands() {
        let lines = |src: &str| text(&seq_transcript(&sequence(&format!("sequenceDiagram\n{src}")), 80));
        // A later `autonumber` restarts the count from there.
        let rows = ["1. A ─▸ B: a", "2. B ─▸ A: b", "100. A ─▸ B: c"];
        assert_eq!(lines("autonumber\nA->>B: a\nB->>A: b\nautonumber 100\nA->>B: c"), rows);
        // Messages before the first one get no number, but they count.
        assert_eq!(lines("A->>B: a\nB->>A: b\nautonumber 10\nA->>B: c"), ["A ─▸ B: a", "B ─▸ A: b", "10. A ─▸ B: c"]);
        assert_eq!(lines("A->>B: a\nB->>A: b\nautonumber\nA->>B: c"), ["A ─▸ B: a", "B ─▸ A: b", "3. A ─▸ B: c"]);
        // `autonumber off` hides the numbers; the count goes on.
        let src = "autonumber 10 5\nA->>B: a\nautonumber off\nB->>A: b\nautonumber\nA->>B: c";
        assert_eq!(lines(src), ["10. A ─▸ B: a", "B ─▸ A: b", "20. A ─▸ B: c"]);
        assert_eq!(lines("autonumber off\nA->>B: a\nB->>A: b"), ["A ─▸ B: a", "B ─▸ A: b"]);
        // A start alone sets the step back to 1; a step of 0 keeps the last.
        let src = "autonumber 1 10\nA->>B: a\nautonumber 50\nB->>A: b\nA->>B: c\nautonumber 7 0\nA->>B: d\nA->>B: e";
        assert_eq!(lines(src), ["1. A ─▸ B: a", "50. B ─▸ A: b", "51. A ─▸ B: c", "7. A ─▸ B: d", "8. A ─▸ B: e"]);
        // Inside blocks, and with notes and activations between messages.
        let src = "A->>B: a\nloop\nautonumber 5\nNote over A: n\nactivate A\nB->>A: b\nend\nA->>B: c";
        assert_eq!(lines(src), ["A ─▸ B: a", "loop", "  Note over A: n", "  5. B ─▸ A: b", "end", "6. A ─▸ B: c"]);
    }

    #[test]
    fn boxes_are_listed_with_their_members() {
        let sd = sequence("sequenceDiagram
            box Aqua Frontend services
            participant W as Web
            participant M as Mobile
            end
            box Backend
            participant API
            end
            box rgb(0, 0, 0)
            actor Ops
            end
            box Green
            end
            box
            end
            W->>API: call
            M->>API: call");
        let d = seq_transcript(&sd, 80);
        assert_eq!(
            text(&d),
            [
                "box Frontend services: Web, Mobile",
                "box Backend: API",
                "box: Ops",
                "1. Web ─▸ API: call",
                "2. Mobile ─▸ API: call",
            ]
        );
        assert_eq!(d.labels.len(), 5);
        // A participant with no line of its own is still listed first.
        let sd = sequence("sequenceDiagram\nparticipant Lone\nbox Team\nparticipant A\nend\nbox Empty title\nend\nA->>B: x");
        let rows = ["Participants: Lone, A, B", "box Team: A", "box Empty title", "1. A ─▸ B: x"];
        assert_eq!(text(&seq_transcript(&sd, 80)), rows);
    }

    #[test]
    fn transcript_shows_each_head() {
        let sd = sequence("sequenceDiagram
            A->B: 1
            A-->B: 2
            A->>B: 3
            A-->>B: 4
            A-xB: 5
            A--xB: 6
            A-)B: 7
            A--)B: 8
            A<<->>B: 9
            A<<-->>B: 10");
        assert_eq!(
            text(&seq_transcript(&sd, 80)),
            [
                "1. A ── B: 1",
                "2. A ┄┄ B: 2",
                "3. A ─▸ B: 3",
                "4. A ┄▸ B: 4",
                "5. A ─✕ B: 5",
                "6. A ┄✕ B: 6",
                "7. A ─▹ B: 7",
                "8. A ┄▹ B: 8",
                "9. A ◂─▸ B: 9",
                "10. A ◂┄▸ B: 10",
            ]
        );
    }

    #[test]
    fn transcript_carries_the_title() {
        let sd = sequence("sequenceDiagram\ntitle Login flow\nA->>B: hi");
        assert_eq!(seq_transcript(&sd, 80).title.as_deref(), Some("Login flow"));
    }

    #[test]
    fn outline_names_the_subgraph_an_edge_ended_at() {
        let mut g = graph("flowchart LR
            client[Browser] --> backend
            subgraph backend [Backend services]
              api[API gateway] --> db[(Postgres)]
              worker[Job worker]
            end
            backend --> cdn[CDN]");
        // The parser attaches these ends to `api`; it also says which
        // subgraph they named.
        g.edges[0].to_cluster = Some(0);
        g.edges[2].from_cluster = Some(0);
        assert_eq!(
            text(&flow_outline(&g, 80)),
            [
                "Backend services: API gateway, Postgres, Job worker",
                "Browser ─▸ Backend services",
                "API gateway ─▸ Postgres",
                "Backend services ─▸ CDN",
                "Job worker",
            ]
        );
        // A subgraph without a title is named by its id, and a member whose
        // only edge was the subgraph's is listed on its own.
        let mut g = graph("flowchart TD\nsubgraph sg\na\nend\nsg --> b");
        g.edges[0].from_cluster = Some(0);
        assert_eq!(text(&flow_outline(&g, 80)), ["sg: a", "sg ─▸ b", "a"]);
    }

    /// The number in the last line, which counts the lines left out.
    fn left_out(d: &Drawing) -> usize {
        let last = d.labels.last().map(|l| l.pieces.iter().map(|p| p.2.as_str()).collect::<String>());
        last.unwrap_or_default().chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap()
    }

    #[test]
    fn long_outlines_stop_at_max_rows() {
        let side = |p: &str| (0..70).map(|i| format!("{p}{i}")).collect::<Vec<_>>().join(" & ");
        let g = graph(&format!("flowchart LR\n{} --> {}", side("a"), side("b")));
        assert_eq!(g.edges.len(), 4_900);
        let d = flow_outline(&g, 80);
        assert_eq!(d.rows.len(), MAX_ROWS + 1);
        assert_eq!(text(&d).last().unwrap(), "2900 more lines not shown");
        let sd = sequence(&format!("sequenceDiagram\n{}", "A->>B: a message\n".repeat(3000)));
        let d = seq_transcript(&sd, 80);
        assert_eq!(text(&d)[MAX_ROWS - 1], "2000. A ─▸ B: a message");
        assert_eq!(text(&d)[MAX_ROWS..], ["1000 more lines not shown"]);
        for w in [1, 2, 7, 30, 120] {
            for d in [flow_outline(&g, w), seq_transcript(&sd, w)] {
                check(&d, w);
                assert!(d.rows.len() <= MAX_ROWS + 25);
                // Every line is either shown or counted.
                let total = if d.kind == Kind::Flowchart { 4_900 } else { 3000 };
                assert_eq!(d.labels.len() - 1 + left_out(&d), total);
            }
        }
        // A first line too long for the cap leaves only the count.
        let g = graph(&format!("flowchart TD\nA[{}] --> B", "x".repeat(MAX_ROWS)));
        let d = flow_outline(&g, 1);
        check(&d, 1);
        assert_eq!(text(&d).concat(), "1linenotshown");
    }

    #[test]
    fn outlines_fit_every_width() {
        let flows = [
            FLOW.to_string(),
            "flowchart LR\nA[認証サービス] -->|トークン検証| B[(データベース)]\nB --> C[😀 emoji 👨‍👩‍👧 family]".into(),
            "flowchart TD\nA[Averyveryveryveryveryveryverylongword] --> B".into(),
            format!(
                "flowchart TD\n{}x[Deep]\n{}",
                (0..12).map(|i| format!("subgraph s{i} [Level {i} group]\n")).collect::<String>(),
                "end\n".repeat(12)
            ),
        ];
        let deep = (0..6).map(|i| format!("alt case {i}\nA->>B: message at depth {i}\n")).collect::<String>()
            + &"end\n".repeat(6);
        let seqs = [
            format!("sequenceDiagram\n{deep}"),
            "sequenceDiagram\n認証->>データベース: 認証サービスのトークン検証\nNote over 認証,データベース: 長いメモ".into(),
            "sequenceDiagram\nparticipant A as A participant with a long name\nactor 認証サービス".into(),
            "sequenceDiagram\nautonumber 4294967295 4294967295\nloop\nA-)B: x\ncreate actor C\nend\ndestroy B".into(),
            concat!(
                "sequenceDiagram\nparticipant Lone\nbox Aqua A long box title for the 認証 group\n",
                "participant 認証サービス\nactor B\nend\nbox\nparticipant C\nend\nautonumber off\n",
                "認証サービス->>B: x\nautonumber 99999999999999999999 7\nB->>C: wrap: y"
            )
            .into(),
        ];
        for w in 1..=120 {
            for src in &flows {
                check(&flow_outline(&graph(src), w), w);
            }
            for src in &seqs {
                check(&seq_transcript(&sequence(src), w), w);
            }
        }
    }
}
