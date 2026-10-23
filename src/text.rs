//! Styled text pieces and word wrapping.

use ratatui::style::Style;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A run of text in one style, optionally part of a link (an index into
/// the layout's link list).
#[derive(Clone, Debug, PartialEq)]
pub struct Seg {
    pub text: String,
    pub style: Style,
    pub link: Option<u32>,
}

impl Seg {
    pub fn new(text: impl Into<String>, style: Style) -> Seg {
        Seg { text: text.into(), style, link: None }
    }

    pub fn plain(text: impl Into<String>) -> Seg {
        Seg::new(text, Style::new())
    }

    pub fn with_link(mut self, link: Option<u32>) -> Seg {
        self.link = link;
        self
    }

    /// Whether text in `style` and `link` can be appended to this run.
    fn joins(&self, style: Style, link: Option<u32>) -> bool {
        self.style == style && self.link == link
    }
}

pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn segs_width(segs: &[Seg]) -> usize {
    segs.iter().map(|s| width(&s.text)).sum()
}

pub fn segs_text(segs: &[Seg]) -> String {
    segs.iter().map(|s| s.text.as_str()).collect()
}

/// Input to the wrapper: text, or a forced line break.
#[derive(Clone, Debug)]
pub enum Tok {
    Text(Seg),
    Break,
}

/// One wrapped line: its segments and the number of words on it.
#[derive(Debug, Default)]
pub struct Wrapped {
    pub segs: Vec<Seg>,
    pub words: u32,
}

/// A word: consecutive non-space text, possibly in several styles.
struct Word {
    pieces: Vec<Seg>,
    width: usize,
    /// Whether whitespace came before this word, and the style and link of
    /// that space (a space inside a link belongs to the link).
    space: Option<(Style, Option<u32>)>,
}

enum Item {
    Word(Word),
    Break,
}

fn split_words(toks: &[Tok]) -> Vec<Item> {
    let mut items = Vec::new();
    let mut cur: Option<Word> = None;
    let mut pending_space: Option<(Style, Option<u32>)> = None;
    for tok in toks {
        match tok {
            Tok::Break => {
                if let Some(w) = cur.take() {
                    items.push(Item::Word(w));
                }
                items.push(Item::Break);
                pending_space = None;
            }
            Tok::Text(seg) => {
                let mut start = None;
                for (i, c) in seg.text.char_indices() {
                    if c == ' ' || c == '\n' || c == '\t' {
                        if let Some(s) = start.take() {
                            push_piece(&mut cur, &seg.text[s..i], seg, &mut pending_space);
                        }
                        if let Some(w) = cur.take() {
                            items.push(Item::Word(w));
                        }
                        pending_space = Some((seg.style, seg.link));
                    } else if start.is_none() {
                        start = Some(i);
                    }
                }
                if let Some(s) = start {
                    push_piece(&mut cur, &seg.text[s..], seg, &mut pending_space);
                }
            }
        }
    }
    if let Some(w) = cur.take() {
        items.push(Item::Word(w));
    }
    items
}

fn push_piece(cur: &mut Option<Word>, text: &str, from: &Seg, space: &mut Option<(Style, Option<u32>)>) {
    let word = cur.get_or_insert_with(|| Word { pieces: Vec::new(), width: 0, space: space.take() });
    word.width += width(text);
    match word.pieces.last_mut() {
        Some(last) if last.joins(from.style, from.link) => last.text.push_str(text),
        _ => word.pieces.push(Seg::new(text, from.style).with_link(from.link)),
    }
}

fn space_seg((style, link): (Style, Option<u32>)) -> Seg {
    Seg::new(" ", style).with_link(link)
}

struct Line {
    segs: Vec<Seg>,
    width: usize,
    words: u32,
}

impl Line {
    fn new() -> Line {
        Line { segs: Vec::new(), width: 0, words: 0 }
    }

    fn push(&mut self, seg: Seg) {
        self.width += width(&seg.text);
        match self.segs.last_mut() {
            Some(last) if last.joins(seg.style, seg.link) => last.text.push_str(&seg.text),
            _ => self.segs.push(seg),
        }
    }

    fn finish(self) -> Wrapped {
        Wrapped { segs: self.segs, words: self.words }
    }
}

/// Greedy word wrap to `width` columns. Words longer than a line are broken
/// between graphemes. Always returns at least one line.
pub fn wrap(toks: &[Tok], width: usize) -> Vec<Wrapped> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = Line::new();
    for item in split_words(toks) {
        let word = match item {
            Item::Break => {
                lines.push(std::mem::replace(&mut line, Line::new()).finish());
                continue;
            }
            Item::Word(w) => w,
        };
        let gap = usize::from(line.width > 0 && word.space.is_some());
        if line.width + gap + word.width <= width {
            if gap == 1 {
                line.push(space_seg(word.space.unwrap_or_default()));
            }
            line.words += 1;
            for p in word.pieces {
                line.push(p);
            }
        } else if word.width <= width {
            lines.push(std::mem::replace(&mut line, Line::new()).finish());
            line.words += 1;
            for p in word.pieces {
                line.push(p);
            }
        } else {
            // Too long for any line: fill the current line, then break.
            if line.width > 0 && line.width + gap + 4 > width {
                lines.push(std::mem::replace(&mut line, Line::new()).finish());
            } else if gap == 1 {
                line.push(space_seg(word.space.unwrap_or_default()));
            }
            line.words += 1;
            for p in word.pieces {
                for g in p.text.graphemes(true) {
                    let gw = self::width(g);
                    if line.width + gw > width && line.width > 0 {
                        lines.push(std::mem::replace(&mut line, Line::new()).finish());
                    }
                    line.push(Seg::new(g, p.style).with_link(p.link));
                }
            }
        }
    }
    lines.push(line.finish());
    lines
}

/// Break a line of code (or raw source) into chunks of at most the given
/// widths. Unlike [`wrap`], every character is kept, so the chunks join back
/// into the original line. Breaks after a space when one is in the second
/// half of the line; otherwise breaks anywhere. Always returns at least one
/// chunk.
pub fn wrap_code(segs: &[Seg], first_width: usize, rest_width: usize) -> Vec<Vec<Seg>> {
    let cells: Vec<(&str, Style, usize, Option<u32>)> = segs
        .iter()
        .flat_map(|s| s.text.graphemes(true).map(move |g| (g, s.style, width(g), s.link)))
        .collect();
    let mut out = Vec::new();
    let mut start = 0;
    let mut limit = first_width.max(1);
    while start < cells.len() {
        let mut used = 0;
        let mut end = start;
        let mut after_space = None;
        while end < cells.len() && (used + cells[end].2 <= limit || end == start) {
            used += cells[end].2;
            if cells[end].0 == " " {
                after_space = Some(end + 1);
            }
            end += 1;
        }
        if end < cells.len()
            && let Some(s) = after_space.filter(|&s| (s - start) * 2 >= limit) {
                end = s;
            }
        let mut line: Vec<Seg> = Vec::new();
        for &(g, style, _, link) in &cells[start..end] {
            match line.last_mut() {
                Some(last) if last.joins(style, link) => last.text.push_str(g),
                _ => line.push(Seg::new(g, style).with_link(link)),
            }
        }
        out.push(line);
        start = end;
        limit = rest_width.max(1);
    }
    if out.is_empty() {
        out.push(Vec::new());
    }
    out
}

/// Pad or truncate `segs` to exactly `w` columns.
pub fn fit(mut segs: Vec<Seg>, w: usize, fill: Style) -> Vec<Seg> {
    let cur = segs_width(&segs);
    if cur < w {
        segs.push(Seg::new(" ".repeat(w - cur), fill));
    }
    segs
}

/// Truncate plain text to `w` columns, adding an ellipsis when cut.
pub fn truncate(s: &str, w: usize) -> String {
    if width(s) <= w {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for g in s.graphemes(true) {
        let gw = width(g);
        if used + gw + 1 > w {
            break;
        }
        out.push_str(g);
        used += gw;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Wrapped]) -> Vec<String> {
        lines.iter().map(|l| segs_text(&l.segs)).collect()
    }

    fn toks(s: &str) -> Vec<Tok> {
        vec![Tok::Text(Seg::plain(s))]
    }

    #[test]
    fn wraps_at_word_boundaries() {
        let lines = wrap(&toks("the quick brown fox jumps over"), 10);
        assert_eq!(text(&lines), vec!["the quick", "brown fox", "jumps over"]);
        assert_eq!(lines[0].words, 2);
    }

    #[test]
    fn keeps_styles_inside_words() {
        use ratatui::style::Modifier;
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let toks = vec![Tok::Text(Seg::plain("a ")), Tok::Text(Seg::new("bo", bold)), Tok::Text(Seg::plain("ld b"))];
        let lines = wrap(&toks, 6);
        assert_eq!(text(&lines), vec!["a bold", "b"]);
        assert_eq!(lines[0].segs.len(), 3);
    }

    #[test]
    fn breaks_long_words() {
        let lines = wrap(&toks("see https://example.com/a/very/long/path ok"), 12);
        assert!(lines.iter().all(|l| segs_width(&l.segs) <= 12));
        assert_eq!(text(&lines).concat().replace(' ', ""), "seehttps://example.com/a/very/long/pathok");
    }

    #[test]
    fn handles_wide_characters() {
        let lines = wrap(&toks("日本語のテキストです"), 7);
        assert!(lines.iter().all(|l| segs_width(&l.segs) <= 7));
    }

    #[test]
    fn hard_breaks_start_new_lines() {
        let toks = vec![Tok::Text(Seg::plain("one")), Tok::Break, Tok::Text(Seg::plain("two"))];
        assert_eq!(text(&wrap(&toks, 20)), vec!["one", "two"]);
    }

    #[test]
    fn code_wraps_after_spaces_and_keeps_every_character() {
        let line = vec![Seg::plain("let total = first_value + second_value;")];
        let chunks: Vec<String> = wrap_code(&line, 20, 18).iter().map(|c| segs_text(c)).collect();
        assert_eq!(chunks, vec!["let total = ", "first_value + ", "second_value;"]);
        assert_eq!(chunks.concat(), "let total = first_value + second_value;");
        // No space in the second half of the line: break anywhere.
        let long = vec![Seg::plain("x = aaaaaaaaaaaaaaaaaaaaaaaa")];
        let chunks: Vec<String> = wrap_code(&long, 10, 10).iter().map(|c| segs_text(c)).collect();
        assert_eq!(chunks, vec!["x = aaaaaa", "aaaaaaaaaa", "aaaaaaaa"]);
    }

    #[test]
    fn truncates_with_ellipsis() {
        assert_eq!(truncate("Installation guide", 8), "Install…");
        assert_eq!(truncate("short", 8), "short");
    }
}
