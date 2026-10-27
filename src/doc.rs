//! The document model: comrak's syntax tree converted into small owned types
//! that the layout engine can walk without borrowing the parser's arena.

use std::collections::HashMap;

use comrak::html::Anchorizer;
use comrak::nodes::{AlertType, AstNode, ListType, NodeValue, TableAlignment};
use comrak::{Arena, Options, parse_document};

pub struct Document {
    pub blocks: Vec<Block>,
}

/// A block plus the 1-based source line it starts on. The line is used to
/// keep the reader's place when the layout changes (resize, raw view).
pub struct Block {
    pub line: usize,
    pub kind: BlockKind,
}

pub enum BlockKind {
    Heading { level: u8, inlines: Vec<Inline>, anchor: String },
    Paragraph(Vec<Inline>),
    Code { lang: String, code: String },
    Quote(Vec<Block>),
    Alert { kind: AlertKind, title: Option<String>, blocks: Vec<Block> },
    List { ordered: bool, start: usize, tight: bool, items: Vec<Item> },
    /// `rows[0]` is the header row.
    Table { aligns: Vec<Align>, rows: Vec<Vec<Vec<Inline>>> },
    Rule,
    /// Text left over after stripping tags from an HTML block.
    Html(String),
    FrontMatter(String),
    Footnote { number: usize, blocks: Vec<Block> },
    Definitions(Vec<Definition>),
    Math(String),
}

pub struct Item {
    /// `Some(checked)` for task list items.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

pub struct Definition {
    pub term: Vec<Inline>,
    pub details: Vec<Vec<Block>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl AlertKind {
    pub fn label(self) -> &'static str {
        match self {
            AlertKind::Note => "Note",
            AlertKind::Tip => "Tip",
            AlertKind::Important => "Important",
            AlertKind::Warning => "Warning",
            AlertKind::Caution => "Caution",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Inline {
    Text(String),
    Code(String),
    Emph(Vec<Inline>),
    Strong(Vec<Inline>),
    Strike(Vec<Inline>),
    Link { url: String, children: Vec<Inline> },
    Image { url: String, alt: String },
    SoftBreak,
    HardBreak,
    FootnoteRef(usize),
    Math(String),
}

/// Plain text of a run of inlines, used for the contents list and anchors.
pub fn plain_text(inlines: &[Inline]) -> String {
    let mut s = String::new();
    collect_text(inlines, &mut s);
    s
}

fn collect_text(inlines: &[Inline], s: &mut String) {
    for i in inlines {
        match i {
            Inline::Text(t) | Inline::Code(t) | Inline::Math(t) => s.push_str(t),
            Inline::Emph(c) | Inline::Strong(c) | Inline::Strike(c) => collect_text(c, s),
            Inline::Link { children, .. } => collect_text(children, s),
            Inline::Image { alt, .. } => s.push_str(alt),
            Inline::SoftBreak | Inline::HardBreak => s.push(' '),
            Inline::FootnoteRef(n) => s.push_str(&format!("[{n}]")),
        }
    }
}

pub fn parse(src: &str) -> Document {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let arena = Arena::new();
    let mut options = Options::default();
    let ext = &mut options.extension;
    ext.strikethrough = true;
    ext.table = true;
    ext.autolink = true;
    ext.tasklist = true;
    ext.footnotes = true;
    ext.description_lists = true;
    ext.front_matter_delimiter = Some("---".into());
    ext.alerts = true;
    ext.math_dollars = true;
    let root = parse_document(&arena, src, &options);
    let mut conv = Converter { anchors: Anchorizer::new(), footnotes: HashMap::new() };
    Document { blocks: conv.blocks(root) }
}

struct Converter {
    anchors: Anchorizer,
    /// Footnote name -> number, assigned in order of first reference.
    footnotes: HashMap<String, usize>,
}

impl Converter {
    fn footnote_number(&mut self, name: &str) -> usize {
        let next = self.footnotes.len() + 1;
        *self.footnotes.entry(name.to_string()).or_insert(next)
    }

    fn blocks<'a>(&mut self, node: &'a AstNode<'a>) -> Vec<Block> {
        let mut out = Vec::new();
        for child in node.children() {
            self.block(child, &mut out);
        }
        out
    }

    fn block<'a>(&mut self, node: &'a AstNode<'a>, out: &mut Vec<Block>) {
        let data = node.data();
        let line = data.sourcepos.start.line;
        let kind = match &data.value {
            NodeValue::FrontMatter(text) => {
                let body: Vec<&str> =
                    text.lines().filter(|l| l.trim() != "---" && l.trim() != "...").collect();
                let body = body.join("\n");
                if body.trim().is_empty() {
                    return;
                }
                BlockKind::FrontMatter(clean(body.trim_matches('\n')))
            }
            NodeValue::Heading(h) => {
                let inlines = self.inlines(node);
                if plain_text(&inlines).trim().is_empty() {
                    return;
                }
                let anchor = self.anchors.anchorize(&plain_text(&inlines));
                BlockKind::Heading { level: h.level, inlines, anchor }
            }
            NodeValue::Paragraph => {
                let inlines = self.inlines(node);
                // A paragraph holding only display math is shown as a math block.
                if let [Inline::Math(tex)] = inlines.as_slice()
                    && is_display_math(node) {
                        out.push(Block { line, kind: BlockKind::Math(tex.clone()) });
                        return;
                    }
                if inlines.is_empty() {
                    return;
                }
                BlockKind::Paragraph(inlines)
            }
            NodeValue::CodeBlock(cb) => {
                let lang = cb.info.split_whitespace().next().unwrap_or("").to_string();
                let code = clean(cb.literal.strip_suffix('\n').unwrap_or(&cb.literal));
                BlockKind::Code { lang, code }
            }
            NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => {
                BlockKind::Quote(self.blocks(node))
            }
            NodeValue::Alert(a) => BlockKind::Alert {
                kind: match a.alert_type {
                    AlertType::Note => AlertKind::Note,
                    AlertType::Tip => AlertKind::Tip,
                    AlertType::Important => AlertKind::Important,
                    AlertType::Warning => AlertKind::Warning,
                    AlertType::Caution => AlertKind::Caution,
                },
                title: a.title.as_deref().map(clean),
                blocks: self.blocks(node),
            },
            NodeValue::List(l) => {
                let mut items = Vec::new();
                for item in node.children() {
                    let task = match &item.data().value {
                        NodeValue::TaskItem(t) => Some(t.symbol.is_some()),
                        _ => None,
                    };
                    items.push(Item { task, blocks: self.blocks(item) });
                }
                BlockKind::List {
                    ordered: l.list_type == ListType::Ordered,
                    start: l.start,
                    tight: l.tight,
                    items,
                }
            }
            NodeValue::Table(t) => {
                let aligns = t
                    .alignments
                    .iter()
                    .map(|a| match a {
                        TableAlignment::Center => Align::Center,
                        TableAlignment::Right => Align::Right,
                        _ => Align::Left,
                    })
                    .collect();
                let rows = node
                    .children()
                    .map(|row| row.children().map(|cell| self.inlines(cell)).collect())
                    .collect();
                BlockKind::Table { aligns, rows }
            }
            NodeValue::ThematicBreak => BlockKind::Rule,
            NodeValue::HtmlBlock(h) => {
                let text = strip_html(&h.literal);
                if text.trim().is_empty() {
                    return;
                }
                BlockKind::Html(text)
            }
            NodeValue::FootnoteDefinition(f) => {
                let number = self.footnote_number(&f.name);
                BlockKind::Footnote { number, blocks: self.blocks(node) }
            }
            NodeValue::DescriptionList => {
                let mut defs = Vec::new();
                for item in node.children() {
                    let mut term = Vec::new();
                    let mut details = Vec::new();
                    for part in item.children() {
                        match part.data().value {
                            NodeValue::DescriptionTerm => {
                                // The term holds a paragraph; take its inlines.
                                for p in part.children() {
                                    term.extend(self.inlines(p));
                                }
                            }
                            NodeValue::DescriptionDetails => details.push(self.blocks(part)),
                            _ => {}
                        }
                    }
                    defs.push(Definition { term, details });
                }
                BlockKind::Definitions(defs)
            }
            // Anything else that holds blocks (directives, subtext, ...): keep the content.
            _ => {
                drop(data);
                out.extend(self.blocks(node));
                return;
            }
        };
        out.push(Block { line, kind });
    }

    fn inlines<'a>(&mut self, node: &'a AstNode<'a>) -> Vec<Inline> {
        let mut out = Vec::new();
        for child in node.children() {
            self.inline(child, &mut out);
        }
        out
    }

    fn inline<'a>(&mut self, node: &'a AstNode<'a>, out: &mut Vec<Inline>) {
        let data = node.data();
        let inline = match &data.value {
            NodeValue::Text(t) => Inline::Text(clean(t)),
            NodeValue::Code(c) => Inline::Code(clean(&c.literal)),
            NodeValue::Emph => Inline::Emph(self.inlines(node)),
            NodeValue::Strong => Inline::Strong(self.inlines(node)),
            NodeValue::Strikethrough => Inline::Strike(self.inlines(node)),
            NodeValue::Link(l) => Inline::Link { url: clean(&l.url), children: self.inlines(node) },
            NodeValue::Image(l) => {
                let alt = plain_text(&self.inlines(node));
                Inline::Image { url: clean(&l.url), alt }
            }
            NodeValue::SoftBreak => Inline::SoftBreak,
            NodeValue::LineBreak => Inline::HardBreak,
            NodeValue::FootnoteReference(f) => Inline::FootnoteRef(self.footnote_number(&f.name)),
            NodeValue::Math(m) => Inline::Math(clean(&m.literal)),
            NodeValue::HtmlInline(html) => {
                let tag = html.trim_start_matches('<').trim_start_matches('/').to_ascii_lowercase();
                if tag.starts_with("br") {
                    out.push(Inline::HardBreak);
                }
                return;
            }
            NodeValue::Raw(t) => Inline::Text(clean(t)),
            // Unknown inline containers (underline, highlight, ...): keep their text.
            _ => {
                drop(data);
                let children = self.inlines(node);
                out.extend(children);
                return;
            }
        };
        out.push(inline);
    }
}

fn is_display_math<'a>(paragraph: &'a AstNode<'a>) -> bool {
    paragraph
        .first_child()
        .map(|c| matches!(&c.data().value, NodeValue::Math(m) if m.display_math))
        .unwrap_or(false)
}

/// Replace control characters so a document can't send escape sequences to
/// the terminal. Tabs and newlines are kept; carriage returns are dropped.
pub fn clean(s: &str) -> String {
    s.chars()
        .filter(|&c| c != '\r')
        .map(|c| match c {
            '\t' | '\n' => c,
            c if c.is_control() => '\u{fffd}',
            c => c,
        })
        .collect()
}

/// Like [`clean`], for text shown on one line (file names, paths): every
/// control character, newlines and tabs included, becomes U+FFFD.
pub fn clean_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { '\u{fffd}' } else { c }).collect()
}

/// Turn an HTML block into readable text: drop tags and comments, keep image
/// alt text, turn block-level tags into line breaks, decode common entities.
pub fn strip_html(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map(|e| &after[e + 3..]).unwrap_or("");
            continue;
        }
        let Some(end) = rest.find('>') else {
            out.push_str(rest);
            rest = "";
            break;
        };
        let tag = &rest[1..end];
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        match name.as_str() {
            "img" => {
                if let Some(alt) = attr(tag, "alt").filter(|a| !a.is_empty()) {
                    out.push_str(&format!("[image: {alt}]"));
                }
            }
            "br" | "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "tr" | "summary"
            | "details" | "table" | "ul" | "ol" | "hr" | "section" | "header" | "footer" => {
                out.push('\n')
            }
            "td" | "th" => out.push(' '),
            _ => {}
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    let decoded = decode_entities(&out);
    let lines: Vec<&str> =
        decoded.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    clean(&lines.join("\n"))
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let pos = lower.find(&format!("{name}="))?;
    let value = &tag[pos + name.len() + 1..];
    let quote = value.chars().next()?;
    if quote == '"' || quote == '\'' {
        let inner = &value[1..];
        inner.find(quote).map(|e| inner[..e].to_string())
    } else {
        Some(value.split(|c: char| c.is_whitespace() || c == '>').next()?.to_string())
    }
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_html_and_keeps_alt_text() {
        let html = "<p align=\"center\">\n  <img src=\"logo.png\" alt=\"Logo\">\n</p>\n<!-- hidden -->\n<h1>Title &amp; more</h1>";
        assert_eq!(strip_html(html), "[image: Logo]\nTitle & more");
    }

    #[test]
    fn control_characters_are_replaced() {
        assert_eq!(clean("a\u{1b}[31mb\tc"), "a\u{fffd}[31mb\tc");
    }

    #[test]
    fn footnotes_are_numbered_by_first_reference() {
        let doc = parse("b[^b] a[^a]\n\n[^a]: A\n[^b]: B\n");
        let numbers: Vec<usize> = doc
            .blocks
            .iter()
            .filter_map(|b| match b.kind {
                BlockKind::Footnote { number, .. } => Some(number),
                _ => None,
            })
            .collect();
        assert_eq!(numbers, vec![1, 2]);
    }
}
