//! Search over the rendered text. Each flow (paragraph, cell, code line) is
//! joined back into one string, so a phrase that wraps across lines is
//! still found.

use crate::layout::{Flow, Rendered};
use crate::text::width;

/// One match, as one or more highlighted runs: (line, column, width).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub spans: Vec<(usize, usize, usize)>,
}

impl Match {
    pub fn line(&self) -> usize {
        self.spans[0].0
    }
}

/// Find every match of `query`. Matching ignores case unless the query has
/// an uppercase letter ("smart case").
pub fn find(r: &Rendered, query: &str) -> Vec<Match> {
    if query.is_empty() {
        return Vec::new();
    }
    let smart = query.chars().any(char::is_uppercase);
    let fold = |c: char| if smart { c } else { c.to_lowercase().next().unwrap_or(c) };
    let needle: Vec<char> = query.chars().map(fold).collect();
    let mut out = Vec::new();
    for flow in &r.flows {
        // Haystack characters, each mapped to (piece, byte offset, byte len)
        // or `None` for the joiner between pieces.
        let mut hay = Vec::new();
        let mut map: Vec<Option<(usize, usize, usize)>> = Vec::new();
        for (pi, piece) in flow.pieces.iter().enumerate() {
            if pi > 0 {
                for c in flow.joiner.chars() {
                    hay.push(c);
                    map.push(None);
                }
            }
            for (bi, c) in piece.text.char_indices() {
                // Non-breaking spaces only pad inline code; skip them.
                if c == '\u{a0}' {
                    continue;
                }
                hay.push(fold(c));
                map.push(Some((pi, bi, c.len_utf8())));
            }
        }
        let n = needle.len();
        let mut i = 0;
        while i + n <= hay.len() {
            if hay[i..i + n] == needle[..] {
                if let Some(m) = to_match(flow, &map[i..i + n]) {
                    out.push(m);
                }
                i += n;
            } else {
                i += 1;
            }
        }
    }
    out.sort_by_key(|m| (m.spans[0].0, m.spans[0].1));
    out
}

fn to_match(flow: &Flow, chars: &[Option<(usize, usize, usize)>]) -> Option<Match> {
    let mut spans = Vec::new();
    let mut cur: Option<(usize, usize, usize)> = None; // (piece, start, end)
    for &(pi, start, len) in chars.iter().flatten() {
        match cur {
            Some((p, s, _)) if p == pi => cur = Some((p, s, start + len)),
            _ => {
                if let Some(c) = cur.take() {
                    spans.push(span(flow, c));
                }
                cur = Some((pi, start, start + len));
            }
        }
    }
    if let Some(c) = cur {
        spans.push(span(flow, c));
    }
    (!spans.is_empty()).then_some(Match { spans })
}

fn span(flow: &Flow, (pi, start, end): (usize, usize, usize)) -> (usize, usize, usize) {
    let piece = &flow.pieces[pi];
    let col = piece.col + width(&piece.text[..start]);
    (piece.line, col, width(&piece.text[start..end]))
}
