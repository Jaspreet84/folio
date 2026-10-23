//! Layout: turns the document into styled lines for a given width.
//!
//! Layout is a pure function of (document, width, theme). The pager only
//! scrolls the result, so resizing means running layout again. Besides the
//! lines, layout records the headings (for the contents pane) and the
//! "flows" of text (for search).

use ratatui::style::{Modifier, Style};

use crate::doc::{Align, Block, BlockKind, Definition, Document, Inline, Item, plain_text};
use crate::highlight::Highlighter;
use crate::table::{MIN_CAP, column_widths};
use crate::text::{Seg, Tok, fit, wrap_code, segs_text, segs_width, truncate, width, wrap};
use crate::theme::Theme;

/// Below this many columns, nested blocks stop indenting further.
const MIN_INNER: usize = 8;

/// One rendered line. `src` is the source line of the block that produced
/// it; `words` counts the words on it, for the reading-time estimate.
#[derive(Clone, Debug, Default)]
pub struct RLine {
    pub segs: Vec<Seg>,
    pub src: usize,
    pub words: u32,
}

/// Text of one flow (a paragraph, table cell, heading or code line) as it
/// was placed on screen: each piece is a run of text on one line.
#[derive(Clone, Debug)]
pub struct Piece {
    pub line: usize,
    pub col: usize,
    pub text: String,
}

/// A flow's pieces joined back together for search. `joiner` is what the
/// wrapping removed between pieces: a space for prose, nothing for code.
#[derive(Clone, Debug)]
pub struct Flow {
    pub pieces: Vec<Piece>,
    pub joiner: &'static str,
}

#[derive(Clone, Debug)]
pub struct HeadingRef {
    pub level: u8,
    pub text: String,
    pub anchor: String,
    /// Rendered line index.
    pub line: usize,
    pub src: usize,
}

/// Something the reader can move focus to with Tab and act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetKind {
    /// A link; the number indexes [`Rendered::links`].
    Link(u32),
    Code { lang: String, code: String },
}

/// A focus target and where it is drawn, as (line, column, width) runs.
#[derive(Clone, Debug)]
pub struct Target {
    pub kind: TargetKind,
    pub spans: Vec<(usize, usize, usize)>,
}

impl Target {
    pub fn first_line(&self) -> usize {
        self.spans.iter().map(|s| s.0).min().unwrap_or(0)
    }

    pub fn last_line(&self) -> usize {
        self.spans.iter().map(|s| s.0).max().unwrap_or(0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Rendered {
    pub width: usize,
    pub lines: Vec<RLine>,
    pub flows: Vec<Flow>,
    pub headings: Vec<HeadingRef>,
    /// Link URLs, as written in the document. `Seg::link` indexes this.
    pub links: Vec<String>,
    /// Focus targets in reading order.
    pub targets: Vec<Target>,
    /// Named places other than headings (footnotes), as (name, line).
    pub anchors: Vec<(String, usize)>,
}

impl Rendered {
    fn push(&mut self, segs: Vec<Seg>, src: usize, words: u32) -> usize {
        self.lines.push(RLine { segs, src, words });
        self.lines.len() - 1
    }

    fn blank(&mut self, src: usize) {
        self.push(Vec::new(), src, 0);
    }

    /// Append `child`, putting `first` before its first line and `rest`
    /// before the others. Both prefixes must have the same width.
    fn nest(&mut self, child: Rendered, first: &[Seg], rest: &[Seg]) {
        let base = self.lines.len();
        let shift = segs_width(first);
        debug_assert_eq!(shift, segs_width(rest));
        for (i, mut line) in child.lines.into_iter().enumerate() {
            let prefix = if i == 0 { first } else { rest };
            if !prefix.is_empty() {
                let mut segs = prefix.to_vec();
                segs.append(&mut line.segs);
                line.segs = segs;
            }
            self.lines.push(line);
        }
        for mut flow in child.flows {
            for p in &mut flow.pieces {
                p.line += base;
                p.col += shift;
            }
            self.flows.push(flow);
        }
        for mut h in child.headings {
            h.line += base;
            self.headings.push(h);
        }
        for mut t in child.targets {
            for s in &mut t.spans {
                s.0 += base;
                s.1 += shift;
            }
            self.targets.push(t);
        }
        for (name, line) in child.anchors {
            self.anchors.push((name, line + base));
        }
    }

    /// Record a target for every link, from where its text ended up.
    fn collect_links(&mut self) {
        let mut spans: Vec<Vec<(usize, usize, usize)>> = vec![Vec::new(); self.links.len()];
        for (i, line) in self.lines.iter().enumerate() {
            let mut col = 0;
            for seg in &line.segs {
                let w = width(&seg.text);
                if let Some(id) = seg.link {
                    let list = &mut spans[id as usize];
                    match list.last_mut() {
                        Some(last) if last.0 == i && last.1 + last.2 == col => last.2 += w,
                        _ => list.push((i, col, w)),
                    }
                }
                col += w;
            }
        }
        for (id, spans) in spans.into_iter().enumerate() {
            if !spans.is_empty() {
                self.targets.push(Target { kind: TargetKind::Link(id as u32), spans });
            }
        }
        self.targets.sort_by_key(|t| (t.first_line(), t.spans[0].1));
    }

    /// Where the line `top` sits in the source: the source line, and how
    /// many rendered lines into that source line's output it is.
    pub fn anchor_at(&self, top: usize) -> (usize, usize) {
        let Some(line) = self.lines.get(top) else { return (usize::MAX, 0) };
        let mut first = top;
        while first > 0 && self.lines[first - 1].src == line.src {
            first -= 1;
        }
        (line.src, top - first)
    }

    /// The rendered line for an anchor from [`Rendered::anchor_at`], possibly
    /// taken from a different layout of the same document.
    pub fn line_for(&self, anchor: (usize, usize)) -> usize {
        let (src, offset) = anchor;
        let found = self
            .lines
            .iter()
            .position(|l| l.src == src)
            .or_else(|| self.lines.iter().position(|l| l.src > src && l.src != usize::MAX));
        let Some(start) = found else { return self.lines.len().saturating_sub(1) };
        let run = self.lines[start..].iter().take_while(|l| l.src == self.lines[start].src).count();
        start + offset.min(run.saturating_sub(1))
    }

    /// The line of the heading or footnote called `name` (ignoring case).
    pub fn find_anchor(&self, name: &str) -> Option<usize> {
        let name = name.to_lowercase();
        let heading = self.headings.iter().find(|h| h.anchor.to_lowercase() == name).map(|h| h.line);
        heading.or_else(|| self.anchors.iter().find(|(a, _)| *a == name).map(|(_, line)| *line))
    }
}

/// Lay out a document at `width` columns.
pub fn layout(doc: &Document, width: usize, theme: &Theme, hl: &mut Highlighter) -> Rendered {
    let width = width.max(1);
    let mut l = Layouter { theme, hl, list_depth: 0, links: Vec::new() };
    let mut out = l.blocks(&doc.blocks, width, false);
    out.width = width;
    out.links = l.links;
    out.collect_links();
    out
}

/// The document's headings in order, without laying anything out. The
/// `line` fields are 0; [`layout_raw`] fills them in for the raw view.
pub fn outline(doc: &Document) -> Vec<HeadingRef> {
    fn walk(blocks: &[Block], out: &mut Vec<HeadingRef>) {
        for b in blocks {
            match &b.kind {
                BlockKind::Heading { level, inlines, anchor } => out.push(HeadingRef {
                    level: *level,
                    text: plain_text(inlines).trim().to_string(),
                    anchor: anchor.clone(),
                    line: 0,
                    src: b.line,
                }),
                BlockKind::Quote(inner) | BlockKind::Alert { blocks: inner, .. } | BlockKind::Footnote { blocks: inner, .. } => {
                    walk(inner, out)
                }
                BlockKind::List { items, .. } => items.iter().for_each(|i| walk(&i.blocks, out)),
                BlockKind::Definitions(defs) => defs.iter().flat_map(|d| &d.details).for_each(|d| walk(d, out)),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&doc.blocks, &mut out);
    out
}

/// Lay out the Markdown source itself, highlighted, for the raw view.
/// `headings` from the rendered view are mapped onto the source lines.
pub fn layout_raw(
    source: &str,
    headings: &[HeadingRef],
    width: usize,
    theme: &Theme,
    hl: &mut Highlighter,
) -> Rendered {
    let width = width.max(1);
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let source = crate::doc::clean(source);
    let source = source.strip_suffix('\n').unwrap_or(&source);
    let plain: Vec<Vec<Seg>> = source.split('\n').map(|l| vec![Seg::plain(expand_tabs(l))]).collect();
    let expanded: String = plain.iter().map(|l| segs_text(l)).collect::<Vec<_>>().join("\n");
    let lines = hl.highlight("md", &expanded, theme).unwrap_or(plain);
    let mut out = Rendered { width, ..Rendered::default() };
    let mut first_line = Vec::with_capacity(lines.len());
    for (i, segs) in lines.into_iter().enumerate() {
        let src = i + 1;
        first_line.push(out.lines.len());
        let marker = if width > 4 { 2 } else { 0 };
        let chunks = wrap_code(&segs, width, width - marker);
        let mut flow = Flow { pieces: Vec::new(), joiner: "" };
        for (j, chunk) in chunks.into_iter().enumerate() {
            let text = segs_text(&chunk);
            let words = text.split_whitespace().count() as u32;
            let mut line = Vec::new();
            let col = if j > 0 && marker > 0 {
                line.push(Seg::new("↪ ", theme.wrap_marker));
                2
            } else {
                0
            };
            line.extend(chunk);
            let idx = out.push(line, src, words);
            flow.pieces.push(Piece { line: idx, col, text });
        }
        out.flows.push(flow);
    }
    for h in headings {
        if let Some(&line) = first_line.get(h.src.saturating_sub(1)) {
            out.headings.push(HeadingRef { line, ..h.clone() });
        }
    }
    out
}

struct Layouter<'t, 'h> {
    theme: &'t Theme,
    hl: &'h mut Highlighter,
    list_depth: usize,
    links: Vec<String>,
}

impl Layouter<'_, '_> {
    fn blocks(&mut self, blocks: &[Block], width: usize, tight: bool) -> Rendered {
        let mut out = Rendered::default();
        let mut seen_footnote = false;
        for block in blocks {
            let child = self.block(block, width);
            if child.lines.is_empty() {
                continue;
            }
            if !out.lines.is_empty() && !tight {
                out.blank(block.line);
            }
            if matches!(block.kind, BlockKind::Footnote { .. }) && !seen_footnote {
                seen_footnote = true;
                let label = " Footnotes ";
                let mut segs = vec![Seg::new("──", self.theme.rule), Seg::new(label, self.theme.dim)];
                let used = 2 + width_of(label);
                if width > used {
                    segs.push(Seg::new("─".repeat(width - used), self.theme.rule));
                }
                if width >= used {
                    out.push(segs, block.line, 0);
                    out.blank(block.line);
                }
            }
            out.nest(child, &[], &[]);
        }
        out
    }

    fn block(&mut self, block: &Block, width: usize) -> Rendered {
        let src = block.line;
        let t = self.theme;
        match &block.kind {
            BlockKind::Heading { level, inlines, anchor } => self.heading(*level, inlines, anchor, width, src),
            BlockKind::Paragraph(inlines) => {
                let mut toks = Vec::new();
                self.inlines(inlines, Style::new(), &mut toks);
                paragraph(&toks, width, src)
            }
            BlockKind::Code { lang, code } => self.code(lang, lang, code, width, src),
            BlockKind::FrontMatter(text) => self.code("yaml", "front matter", text, width, src),
            BlockKind::Quote(blocks) => {
                let bar = [Seg::new("│ ", t.quote_bar)];
                self.nested(blocks, width, false, (&bar, &bar), src)
            }
            BlockKind::Alert { kind, title, blocks } => {
                let style = t.alert[*kind as usize];
                let bar = [Seg::new("┃ ", style)];
                let Some(inner) = inner_width(width, 2) else {
                    return self.blocks(blocks, width, false);
                };
                let mut child = Rendered::default();
                let label = title.clone().unwrap_or_else(|| kind.label().to_string());
                let label_toks = [Tok::Text(Seg::new(label, style.add_modifier(Modifier::BOLD)))];
                child.nest(paragraph(&label_toks, inner, src), &[], &[]);
                let body = self.blocks(blocks, inner, false);
                child.nest(body, &[], &[]);
                let mut out = Rendered::default();
                out.nest(child, &bar, &bar);
                out
            }
            BlockKind::List { ordered, start, tight, items } => {
                self.list(*ordered, *start, *tight, items, width, src)
            }
            BlockKind::Table { aligns, rows } => self.table(aligns, rows, width, src),
            BlockKind::Rule => {
                let mut out = Rendered::default();
                out.push(vec![Seg::new("─".repeat(width), t.rule)], src, 0);
                out
            }
            BlockKind::Html(text) => {
                let mut out = Rendered::default();
                for line in text.lines() {
                    let toks = [Tok::Text(Seg::new(line, t.dim))];
                    out.nest(paragraph(&toks, width, src), &[], &[]);
                }
                out
            }
            BlockKind::Footnote { number, blocks } => {
                let marker = format!("[{number}] ");
                let first = [Seg::new(marker.clone(), t.footnote)];
                let rest = [Seg::plain(" ".repeat(width_of(&marker)))];
                let mut out = self.nested(blocks, width, true, (&first, &rest), src);
                out.anchors.push((footnote_anchor(*number), 0));
                out
            }
            BlockKind::Definitions(defs) => self.definitions(defs, width, src),
            BlockKind::Math(tex) => {
                let mut out = Rendered::default();
                let Some(inner) = inner_width(width, 2) else { return out };
                for line in tex.trim().lines() {
                    let toks = [Tok::Text(Seg::new(line.trim(), t.math))];
                    let pad = [Seg::plain("  ")];
                    let mut child = Rendered::default();
                    child.nest(paragraph(&toks, inner, src), &[], &[]);
                    out.nest(child, &pad, &pad);
                }
                out
            }
        }
    }

    /// Lay out `blocks` indented behind a prefix, or without the prefix when
    /// there is no room for it.
    fn nested(
        &mut self,
        blocks: &[Block],
        width: usize,
        tight: bool,
        (first, rest): (&[Seg], &[Seg]),
        src: usize,
    ) -> Rendered {
        let mut out = Rendered::default();
        match inner_width(width, segs_width(first)) {
            Some(inner) => {
                let child = self.blocks(blocks, inner, tight);
                if child.lines.is_empty() {
                    out.push(first.to_vec(), src, 0);
                } else {
                    out.nest(child, first, rest);
                }
            }
            None => out = self.blocks(blocks, width, tight),
        }
        out
    }

    fn heading(&mut self, level: u8, inlines: &[Inline], anchor: &str, width: usize, src: usize) -> Rendered {
        let t = self.theme;
        let idx = usize::from(level.clamp(1, 6)) - 1;
        let style = t.heading[idx];
        let mut toks = Vec::new();
        if !t.has_color() && level >= 3 {
            toks.push(Tok::Text(Seg::new(format!("{} ", "#".repeat(usize::from(level))), style)));
        }
        self.inlines(inlines, style, &mut toks);
        let mut out = paragraph(&toks, width, src);
        out.headings.push(HeadingRef {
            level,
            text: plain_text(inlines).trim().to_string(),
            anchor: anchor.to_string(),
            line: 0,
            src,
        });
        if level <= 2 {
            let ch = if level == 1 { "━" } else { "─" };
            out.push(vec![Seg::new(ch.repeat(width), t.heading_rule[idx])], src, 0);
        }
        out
    }

    fn code(&mut self, lang: &str, label: &str, code: &str, width: usize, src: usize) -> Rendered {
        let t = self.theme;
        let mut out = Rendered::default();
        let plain: Vec<Vec<Seg>> = code.split('\n').map(|l| vec![Seg::plain(expand_tabs(l))]).collect();
        let expanded: String = plain.iter().map(|l| segs_text(l)).collect::<Vec<_>>().join("\n");
        let lines = self.hl.highlight(lang, &expanded, t).unwrap_or(plain);

        if width < MIN_INNER {
            for segs in lines {
                let mut flow = Flow { pieces: Vec::new(), joiner: "" };
                for chunk in wrap_code(&segs, width, width) {
                    let text = segs_text(&chunk);
                    let idx = out.push(chunk, src, text.split_whitespace().count() as u32);
                    flow.pieces.push(Piece { line: idx, col: 0, text });
                }
                out.flows.push(flow);
            }
            return out;
        }

        let inner = width - 4;
        let border = t.code_border;
        let mut top = vec![Seg::new("╭─", border)];
        let label = truncate(label, inner.saturating_sub(2));
        if !label.is_empty() {
            top.push(Seg::new(format!(" {label} "), t.code_label));
        }
        let used = segs_width(&top);
        top.push(Seg::new(format!("{}╮", "─".repeat(width - used - 1)), border));
        out.push(top, src, 0);

        for segs in lines {
            let mut flow = Flow { pieces: Vec::new(), joiner: "" };
            let chunks = wrap_code(&segs, inner, inner - 2);
            for (j, chunk) in chunks.into_iter().enumerate() {
                let text = segs_text(&chunk);
                let words = text.split_whitespace().count() as u32;
                let mut line = vec![Seg::new("│ ", border)];
                let mut body = Vec::new();
                let col = if j > 0 {
                    body.push(Seg::new("↪ ", t.wrap_marker));
                    4
                } else {
                    2
                };
                body.extend(chunk);
                line.extend(fit(body, inner, Style::new()));
                line.push(Seg::new(" │", border));
                let idx = out.push(line, src, words);
                flow.pieces.push(Piece { line: idx, col, text });
            }
            out.flows.push(flow);
        }
        out.push(vec![Seg::new(format!("╰{}╯", "─".repeat(width - 2)), border)], src, 0);
        // Focus outlines the box: top and bottom borders plus the side bars.
        let last = out.lines.len() - 1;
        let mut spans = vec![(0, 0, width), (last, 0, width)];
        for line in 1..last {
            spans.push((line, 0, 1));
            spans.push((line, width - 1, 1));
        }
        out.targets.push(Target { kind: TargetKind::Code { lang: lang.to_string(), code: code.to_string() }, spans });
        out
    }

    fn list(&mut self, ordered: bool, start: usize, tight: bool, items: &[Item], width: usize, src: usize) -> Rendered {
        const BULLETS: [&str; 4] = ["•", "◦", "▪", "‣"];
        let t = self.theme;
        let mut out = Rendered::default();
        let last = start + items.len().saturating_sub(1);
        let digits = last.to_string().len();
        self.list_depth += 1;
        for (i, item) in items.iter().enumerate() {
            let (marker, style) = match item.task {
                Some(true) => ("☑ ".to_string(), t.task_done),
                Some(false) => ("☐ ".to_string(), t.task_todo),
                None if ordered => (format!("{:>digits$}. ", start + i), t.list_marker),
                None => (format!("{} ", BULLETS[(self.list_depth - 1) % BULLETS.len()]), t.list_marker),
            };
            let first = [Seg::new(marker.clone(), style)];
            let rest = [Seg::plain(" ".repeat(width_of(&marker)))];
            let child = self.nested(&item.blocks, width, tight, (&first, &rest), src);
            if child.lines.is_empty() {
                continue;
            }
            if !out.lines.is_empty() && !tight {
                out.blank(item.blocks.first().map_or(src, |b| b.line));
            }
            out.nest(child, &[], &[]);
        }
        self.list_depth -= 1;
        if out.lines.is_empty() && !items.is_empty() {
            out.push(Vec::new(), src, 0);
        }
        out
    }

    fn definitions(&mut self, defs: &[Definition], width: usize, src: usize) -> Rendered {
        let mut out = Rendered::default();
        for (i, def) in defs.iter().enumerate() {
            if i > 0 {
                out.blank(src);
            }
            let mut toks = Vec::new();
            self.inlines(&def.term, Style::new().add_modifier(Modifier::BOLD), &mut toks);
            out.nest(paragraph(&toks, width, src), &[], &[]);
            let pad = [Seg::plain("    ")];
            for details in &def.details {
                let child = self.nested(details, width, true, (&pad, &pad), src);
                out.nest(child, &[], &[]);
            }
        }
        out
    }

    fn table(&mut self, aligns: &[Align], rows: &[Vec<Vec<Inline>>], width: usize, src: usize) -> Rendered {
        let t = self.theme;
        let ncols = rows.iter().map(Vec::len).max().unwrap_or(0);
        if ncols == 0 {
            return Rendered::default();
        }
        // Each cell as wrap input; the header row is bold.
        let mut cells: Vec<Vec<Vec<Tok>>> = Vec::with_capacity(rows.len());
        for (r, row) in rows.iter().enumerate() {
            let style = if r == 0 { t.table_header } else { Style::new() };
            let mut out_row = Vec::with_capacity(ncols);
            for c in 0..ncols {
                let mut toks = Vec::new();
                if let Some(cell) = row.get(c) {
                    self.inlines(cell, style, &mut toks);
                }
                out_row.push(toks);
            }
            cells.push(out_row);
        }

        let mut min = vec![1usize; ncols];
        let mut max = vec![1usize; ncols];
        for row in &cells {
            for (c, toks) in row.iter().enumerate() {
                let (word, line) = measure(toks);
                min[c] = min[c].max(word.min(MIN_CAP));
                max[c] = max[c].max(line);
            }
        }
        for c in 0..ncols {
            max[c] = max[c].max(min[c]);
        }
        let overhead = 3 * ncols + 1;
        let widths = width.checked_sub(overhead).and_then(|avail| column_widths(&min, &max, avail));
        let Some(widths) = widths else {
            return self.table_records(&cells, rows, width, src);
        };

        let border = t.table_border;
        let rule = |l: &str, m: &str, r: &str, fill: &str| -> Vec<Seg> {
            let inner: Vec<String> = widths.iter().map(|w| fill.repeat(w + 2)).collect();
            vec![Seg::new(format!("{l}{}{r}", inner.join(m)), border)]
        };
        let mut out = Rendered::default();
        out.push(rule("╭", "┬", "╮", "─"), src, 0);

        let wrapped: Vec<Vec<Vec<crate::text::Wrapped>>> = cells
            .iter()
            .map(|row| row.iter().enumerate().map(|(c, toks)| wrap(toks, widths[c])).collect())
            .collect();
        let multiline = wrapped.iter().skip(1).any(|row| row.iter().any(|cell| cell.len() > 1));

        for (r, row) in wrapped.into_iter().enumerate() {
            if r == 1 {
                out.push(rule("╞", "╪", "╡", "═"), src, 0);
            } else if r > 1 && multiline {
                out.push(rule("├", "┼", "┤", "─"), src, 0);
            }
            let height = row.iter().map(Vec::len).max().unwrap_or(1);
            let first_line = out.lines.len();
            let mut lines: Vec<Vec<Seg>> = vec![Vec::new(); height];
            let mut words = vec![0u32; height];
            let mut flows = Vec::new();
            let mut x = 0;
            for (c, cell) in row.into_iter().enumerate() {
                let w = widths[c];
                let align = aligns.get(c).copied().unwrap_or(Align::Left);
                let mut flow = Flow { pieces: Vec::new(), joiner: " " };
                for (k, line) in lines.iter_mut().enumerate() {
                    line.push(Seg::new(if c == 0 { "│ " } else { " │ " }, border));
                    let content_x = x + if c == 0 { 2 } else { 3 };
                    match cell.get(k) {
                        Some(wl) => {
                            let cw = segs_width(&wl.segs);
                            let pad = w.saturating_sub(cw);
                            let left = match align {
                                Align::Left => 0,
                                Align::Center => pad / 2,
                                Align::Right => pad,
                            };
                            if left > 0 {
                                line.push(Seg::plain(" ".repeat(left)));
                            }
                            line.extend(wl.segs.iter().cloned());
                            if pad - left > 0 {
                                line.push(Seg::plain(" ".repeat(pad - left)));
                            }
                            words[k] += wl.words;
                            let text = segs_text(&wl.segs);
                            if !text.is_empty() {
                                flow.pieces.push(Piece { line: first_line + k, col: content_x + left, text });
                            }
                        }
                        None => line.push(Seg::plain(" ".repeat(w))),
                    }
                }
                x += if c == 0 { 2 + w } else { 3 + w };
                if !flow.pieces.is_empty() {
                    flows.push(flow);
                }
            }
            for (k, mut line) in lines.into_iter().enumerate() {
                line.push(Seg::new(" │", border));
                out.push(line, src, words[k]);
            }
            out.flows.extend(flows);
        }
        out.push(rule("╰", "┴", "╯", "─"), src, 0);
        out
    }

    /// Fallback for tables too wide to fit: each row becomes a small record
    /// of `Header: value` lines.
    fn table_records(&mut self, cells: &[Vec<Vec<Tok>>], rows: &[Vec<Vec<Inline>>], width: usize, src: usize) -> Rendered {
        let t = self.theme;
        let mut out = Rendered::default();
        let headers: Vec<String> = (0..cells[0].len())
            .map(|c| {
                let h = rows[0].get(c).map(|cell| plain_text(cell)).unwrap_or_default();
                if h.trim().is_empty() { format!("Column {}", c + 1) } else { h.trim().to_string() }
            })
            .collect();
        let indent = if width > MIN_INNER { 2 } else { 0 };
        for (r, row) in cells.iter().enumerate().skip(1) {
            let label = format!(" {r} ");
            let mut rule = vec![Seg::new("──", t.table_border), Seg::new(label.clone(), t.dim)];
            let used = 2 + width_of(&label);
            if width > used {
                rule.push(Seg::new("─".repeat(width - used), t.table_border));
            }
            if width >= used {
                out.push(rule, src, 0);
            }
            for (c, toks) in row.iter().enumerate() {
                let mut all = vec![Tok::Text(Seg::new(format!("{}: ", headers[c]), t.table_header))];
                all.extend(toks.iter().cloned());
                // Hanging indent: continuation lines are indented under the label.
                let mut flow = Flow { pieces: Vec::new(), joiner: " " };
                for (k, wl) in wrap(&all, width - indent).into_iter().enumerate() {
                    let text = segs_text(&wl.segs);
                    let col = if k == 0 { 0 } else { indent };
                    let mut segs = vec![Seg::plain(" ".repeat(col))];
                    segs.extend(wl.segs);
                    let idx = out.push(segs, src, wl.words);
                    flow.pieces.push(Piece { line: idx, col, text });
                }
                out.flows.push(flow);
            }
        }
        out
    }

    /// Flatten inlines into wrap input, applying `style` and the theme.
    fn inlines(&mut self, inlines: &[Inline], style: Style, out: &mut Vec<Tok>) {
        self.inlines_in(inlines, style, None, out);
    }

    /// Like [`Self::inlines`], with every segment marked as part of `link`.
    fn inlines_in(&mut self, inlines: &[Inline], style: Style, link: Option<u32>, out: &mut Vec<Tok>) {
        let t = self.theme;
        let seg = |text: String, style: Style| Tok::Text(Seg::new(text, style).with_link(link));
        for inline in inlines {
            match inline {
                Inline::Text(s) => out.push(seg(s.clone(), style)),
                Inline::Code(s) => {
                    if t.has_color() {
                        // Non-breaking spaces pad the background without
                        // giving the wrapper a place to break.
                        out.push(seg(format!("\u{a0}{s}\u{a0}"), style.patch(t.code_inline)));
                    } else {
                        out.push(seg(format!("`{s}`"), style));
                    }
                }
                Inline::Emph(c) => self.inlines_in(c, style.add_modifier(Modifier::ITALIC), link, out),
                Inline::Strong(c) => self.inlines_in(c, style.add_modifier(Modifier::BOLD), link, out),
                Inline::Strike(c) => self.inlines_in(c, style.add_modifier(Modifier::CROSSED_OUT), link, out),
                Inline::Link { url, children } => {
                    let id = Some(self.links.len() as u32);
                    self.links.push(url.clone());
                    let link_style = style.patch(t.link);
                    if children.is_empty() {
                        out.push(Tok::Text(Seg::new(url.clone(), link_style).with_link(id)));
                        continue;
                    }
                    self.inlines_in(children, link_style, id, out);
                    if !t.hyperlinks && show_url(url, children) {
                        out.push(seg(format!(" ({url})"), style.patch(t.url)));
                    }
                }
                Inline::Image { alt, .. } => {
                    let alt = if alt.trim().is_empty() { "image" } else { alt.trim() };
                    let text = if t.has_color() { format!("▣ {alt}") } else { format!("[image: {alt}]") };
                    out.push(seg(text, style.patch(t.image)));
                }
                Inline::SoftBreak => out.push(seg(" ".into(), style)),
                Inline::HardBreak => out.push(Tok::Break),
                Inline::FootnoteRef(n) => {
                    // A link to the footnote, so Tab and Enter can jump there.
                    let id = Some(self.links.len() as u32);
                    self.links.push(format!("#{}", footnote_anchor(*n)));
                    out.push(Tok::Text(Seg::new(format!("[{n}]"), style.patch(t.footnote)).with_link(id)));
                }
                Inline::Math(s) => out.push(seg(s.clone(), style.patch(t.math))),
            }
        }
    }
}

/// The anchor name of footnote `n`; footnote references link to it.
pub fn footnote_anchor(n: usize) -> String {
    format!("footnote-{n}")
}

/// Wrap prose into lines and record it as one flow.
fn paragraph(toks: &[Tok], width: usize, src: usize) -> Rendered {
    let mut out = Rendered::default();
    let mut flow = Flow { pieces: Vec::new(), joiner: " " };
    for wl in wrap(toks, width) {
        let text = segs_text(&wl.segs);
        let idx = out.push(wl.segs, src, wl.words);
        if !text.is_empty() {
            flow.pieces.push(Piece { line: idx, col: 0, text });
        }
    }
    out.flows.push(flow);
    out
}

fn inner_width(width: usize, prefix: usize) -> Option<usize> {
    (width >= prefix + MIN_INNER).then(|| width - prefix)
}

fn width_of(s: &str) -> usize {
    width(s)
}

/// Longest word and longest unwrapped line of a cell.
fn measure(toks: &[Tok]) -> (usize, usize) {
    let mut longest_word = 0;
    let mut word = 0;
    for tok in toks {
        match tok {
            Tok::Break => word = 0,
            Tok::Text(seg) => {
                for g in unicode_segmentation::UnicodeSegmentation::graphemes(seg.text.as_str(), true) {
                    if g == " " || g == "\t" || g == "\n" {
                        word = 0;
                    } else {
                        word += width(g);
                        longest_word = longest_word.max(word);
                    }
                }
            }
        }
    }
    let longest_line = wrap(toks, usize::MAX / 2).iter().map(|l| segs_width(&l.segs)).max().unwrap_or(0);
    (longest_word, longest_line)
}

fn show_url(url: &str, children: &[Inline]) -> bool {
    if url.is_empty() || url.starts_with('#') {
        return false;
    }
    if children.iter().all(|c| matches!(c, Inline::Image { .. })) {
        return false;
    }
    let text = plain_text(children);
    let text = text.trim();
    let bare = url.trim_start_matches("mailto:");
    text != url && text != bare && url != format!("https://{text}") && url != format!("http://{text}")
}

/// Expand tabs to 4-column stops.
pub fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return line.to_string();
    }
    let mut out = String::new();
    let mut col = 0;
    for g in unicode_segmentation::UnicodeSegmentation::graphemes(line, true) {
        if g == "\t" {
            let n = 4 - col % 4;
            out.push_str(&" ".repeat(n));
            col += n;
        } else {
            out.push_str(g);
            col += width(g);
        }
    }
    out
}

/// Plain text of every line, for tests and `--color never`.
pub fn to_plain(r: &Rendered) -> String {
    let mut s = String::new();
    for line in &r.lines {
        s.push_str(segs_text(&line.segs).trim_end());
        s.push('\n');
    }
    s
}
