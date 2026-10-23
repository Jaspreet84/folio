use folio::highlight::Highlighter;
use folio::layout::{self, Rendered};
use folio::search;
use folio::text::{segs_text, segs_width};
use folio::theme::{Depth, Theme};
use folio::doc;

const FIXTURES: [&str; 4] = ["basics", "tables", "code", "edge"];

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}.md", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn render(source: &str, width: usize, depth: Depth, hl: &mut Highlighter) -> Rendered {
    let theme = Theme::new(true, depth);
    layout::layout(&doc::parse(source), width, &theme, hl)
}

#[test]
fn snapshots() {
    let mut hl = Highlighter::new();
    for name in FIXTURES {
        let source = fixture(name);
        for width in [40, 80, 120] {
            let plain = render(&source, width, Depth::None, &mut hl);
            insta::assert_snapshot!(format!("{name}_{width}_plain"), layout::to_plain(&plain));
        }
        let color = render(&source, 80, Depth::TrueColor, &mut hl);
        insta::assert_snapshot!(format!("{name}_80_color"), layout::to_plain(&color));
    }
}

#[test]
fn lines_never_exceed_the_width() {
    let mut hl = Highlighter::new();
    for name in FIXTURES {
        let source = fixture(name);
        for depth in [Depth::None, Depth::TrueColor] {
            for width in 20..=140 {
                let r = render(&source, width, depth, &mut hl);
                for (i, line) in r.lines.iter().enumerate() {
                    let w = segs_width(&line.segs);
                    assert!(w <= width, "{name} at width {width}: line {i} is {w} wide: {:?}", segs_text(&line.segs));
                }
            }
        }
    }
}

#[test]
fn raw_view_fits_and_maps_headings() {
    let mut hl = Highlighter::new();
    let theme = Theme::new(true, Depth::TrueColor);
    let source = fixture("basics");
    let rendered = render(&source, 60, Depth::TrueColor, &mut hl);
    let raw = layout::layout_raw(&source, &rendered.headings, 60, &theme, &mut hl);
    assert!(raw.lines.iter().all(|l| segs_width(&l.segs) <= 60));
    assert_eq!(raw.headings.len(), rendered.headings.len());
    for h in &raw.headings {
        let line = segs_text(&raw.lines[h.line].segs);
        assert!(line.starts_with('#'), "heading {:?} mapped to {line:?}", h.text);
    }
}

#[test]
fn no_control_characters_reach_the_output() {
    let mut hl = Highlighter::new();
    let r = render(&fixture("edge"), 80, Depth::TrueColor, &mut hl);
    for line in &r.lines {
        for seg in &line.segs {
            assert!(!seg.text.chars().any(|c| c.is_control()), "control character in {:?}", seg.text);
        }
    }
}

#[test]
fn headings_are_listed_in_order_with_levels() {
    let mut hl = Highlighter::new();
    let r = render(&fixture("basics"), 80, Depth::None, &mut hl);
    let found: Vec<(u8, &str)> = r.headings.iter().map(|h| (h.level, h.text.as_str())).collect();
    assert_eq!(found, vec![(1, "Getting started"), (2, "Install"), (3, "Loose lists")]);
    assert_eq!(r.headings[1].anchor, "install");
    for h in &r.headings {
        assert!(segs_text(&r.lines[h.line].segs).contains(&h.text));
    }
}

#[test]
fn search_finds_phrases_across_wrapped_lines() {
    let mut hl = Highlighter::new();
    let r = render("alpha beta gamma delta epsilon", 12, Depth::None, &mut hl);
    assert_eq!(layout::to_plain(&r), "alpha beta\ngamma delta\nepsilon\n");
    let m = search::find(&r, "beta gamma");
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].spans, vec![(0, 6, 4), (1, 0, 5)]);
}

#[test]
fn search_uses_smart_case() {
    let mut hl = Highlighter::new();
    let r = render("Folio and folio", 80, Depth::None, &mut hl);
    assert_eq!(search::find(&r, "folio").len(), 2);
    assert_eq!(search::find(&r, "Folio").len(), 1);
}

#[test]
fn search_finds_text_in_tables_code_and_inline_code() {
    let mut hl = Highlighter::new();
    let source = "| Key | Action |\n|---|---|\n| j | Scroll down |\n\n```\nlet needle = 1;\n```\n\nUse `cargo build` here.";
    let r = render(source, 60, Depth::TrueColor, &mut hl);
    for (query, expected) in [("scroll down", "Scroll down"), ("needle", "needle"), ("use cargo", "Use \u{a0}cargo")] {
        let m = search::find(&r, query);
        assert_eq!(m.len(), 1, "{query}");
        let (line, col, w) = m[0].spans[0];
        let text = segs_text(&r.lines[line].segs);
        let start: String = text.chars().skip(col).collect();
        assert!(start.starts_with(expected), "{query}: {start:?}");
        assert!(w > 0);
    }
}

#[test]
fn links_and_code_blocks_become_focus_targets() {
    use folio::layout::TargetKind;
    let mut hl = Highlighter::new();
    let source = "See [the install guide](docs/install.md) and [the site](https://example.com).\n\n```sh\necho hi\n```\n\nA note.[^1]\n\n[^1]: Footnote text.\n";
    let r = render(source, 30, Depth::TrueColor, &mut hl);
    let kinds: Vec<String> = r
        .targets
        .iter()
        .map(|t| match &t.kind {
            TargetKind::Link(id) => r.links[*id as usize].clone(),
            TargetKind::Code { lang, .. } => format!("code:{lang}"),
        })
        .collect();
    assert_eq!(kinds, vec!["docs/install.md", "https://example.com", "code:sh", "#footnote-1"]);

    // The first link wraps: its spans cover exactly its text on each line.
    let first = &r.targets[0];
    let covered: String = first
        .spans
        .iter()
        .map(|&(line, col, w)| segs_text(&r.lines[line].segs).chars().skip(col).take(w).collect::<String>())
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(covered, "the install guide");

    // The footnote reference leads to the footnote.
    let line = r.find_anchor("footnote-1").expect("footnote anchor");
    assert!(segs_text(&r.lines[line].segs).starts_with("[1] Footnote text"));
}

#[test]
fn urls_are_hidden_when_the_terminal_shows_hyperlinks() {
    let mut hl = Highlighter::new();
    let doc = doc::parse("Read [the docs](https://example.com/docs).");
    let mut theme = Theme::new(true, Depth::TrueColor);
    let plain = layout::to_plain(&layout::layout(&doc, 80, &theme, &mut hl));
    assert_eq!(plain.trim(), "Read the docs (https://example.com/docs).");
    theme.hyperlinks = true;
    let linked = layout::to_plain(&layout::layout(&doc, 80, &theme, &mut hl));
    assert_eq!(linked.trim(), "Read the docs.");
}

#[test]
fn outline_matches_the_layout_headings() {
    let mut hl = Highlighter::new();
    let source = fixture("basics");
    let r = render(&source, 80, Depth::None, &mut hl);
    let outline = layout::outline(&doc::parse(&source));
    let a: Vec<(&str, usize)> = outline.iter().map(|h| (h.text.as_str(), h.src)).collect();
    let b: Vec<(&str, usize)> = r.headings.iter().map(|h| (h.text.as_str(), h.src)).collect();
    assert_eq!(a, b);
}

#[test]
fn position_survives_a_relayout() {
    let mut hl = Highlighter::new();
    let source = fixture("basics");
    let narrow = render(&source, 40, Depth::None, &mut hl);
    let wide = render(&source, 120, Depth::None, &mut hl);
    let install = &narrow.headings[1];
    let anchor = narrow.anchor_at(install.line);
    assert_eq!(wide.line_for(anchor), wide.headings[1].line);
}
