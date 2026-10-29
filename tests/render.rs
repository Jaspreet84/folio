use folio::diagram::Diagrams;
use folio::highlight::Highlighter;
use folio::layout::{self, Rendered};
use folio::search;
use folio::text::{segs_text, segs_width};
use folio::theme::{Depth, Theme};
use folio::doc;

const FIXTURES: [&str; 5] = ["basics", "tables", "code", "edge", "diagrams"];

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}.md", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn render(source: &str, width: usize, depth: Depth, hl: &mut Highlighter) -> Rendered {
    let theme = Theme::new(true, depth);
    layout::layout(&doc::parse(source), width, &theme, hl, &mut Diagrams::default())
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
            TargetKind::Diagram(id) => format!("diagram:{}", r.diagrams[*id as usize].kind.unwrap_or("?")),
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
    let plain = layout::to_plain(&layout::layout(&doc, 80, &theme, &mut hl, &mut Diagrams::default()));
    assert_eq!(plain.trim(), "Read the docs (https://example.com/docs).");
    theme.hyperlinks = true;
    let linked = layout::to_plain(&layout::layout(&doc, 80, &theme, &mut hl, &mut Diagrams::default()));
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

#[test]
fn mermaid_fences_become_diagrams() {
    use folio::diagram::Mode;
    use folio::layout::TargetKind;
    let mut hl = Highlighter::new();
    let theme = Theme::new(true, Depth::None);
    let source = "```mermaid\nflowchart LR\n  A --> B\n```\n\n```{Mermaid}\nflowchart LR\n  A --> B\n```\n\n```mmd\nflowchart LR\n```\n\n    indented\n";
    let doc = doc::parse(source);
    let r = layout::layout(&doc, 60, &theme, &mut hl, &mut Diagrams::default());
    let diagrams: Vec<bool> = r.targets.iter().map(|t| matches!(t.kind, TargetKind::Diagram(_))).collect();
    assert_eq!(diagrams, [true, true, false, false]);
    assert_eq!(r.diagrams.len(), 2);
    // Identical diagrams are told apart by their occurrence number.
    assert_eq!(r.diagrams[0].key.0, r.diagrams[1].key.0);
    assert_eq!((r.diagrams[0].key.1, r.diagrams[1].key.1), (0, 1));

    // `--diagrams source` shows today's code boxes.
    let r = layout::layout(&doc, 60, &theme, &mut hl, &mut Diagrams::new(Mode::Source));
    assert!(r.targets.iter().all(|t| matches!(t.kind, TargetKind::Code { .. })));
    assert!(r.diagrams.is_empty());
}

#[test]
fn an_unclosed_diagram_shows_its_source() {
    let mut hl = Highlighter::new();
    let r = render("```mermaid\nflowchart LR\n  A --> B\n", 40, Depth::None, &mut hl);
    let text = layout::to_plain(&r);
    assert!(text.contains("│   A --> B"), "{text}");
    assert!(text.contains("Not drawn: the code fence isn't closed"), "{text}");

    // In a quote or list item at the end of the document, too.
    let r = render("- item\n\n  ```mermaid\n  flowchart TD\n    A --> B\n\n", 40, Depth::None, &mut hl);
    let text = layout::to_plain(&r);
    assert!(text.contains("Not drawn: the code fence isn't closed"), "{text}");
}

#[test]
fn a_diagram_ended_by_its_quote_or_list_item_is_drawn() {
    use folio::layout::Shown;
    let mut hl = Highlighter::new();
    for source in [
        "> ```mermaid\n> flowchart TD\n>   C --> D\n\nAfter quote.\n",
        "- item\n\n  ```mermaid\n  flowchart TD\n    C --> D\n\nParagraph after the list.\n",
    ] {
        let r = render(source, 50, Depth::None, &mut hl);
        let text = layout::to_plain(&r);
        assert_eq!(r.diagrams.len(), 1, "{text}");
        assert_eq!(r.diagrams[0].shown, Shown::Drawing, "{text}");
        assert_eq!(r.diagrams[0].source, "flowchart TD\n  C --> D");
        assert!(!text.contains("Not drawn"), "{text}");
        assert!(text.contains("mermaid · flowchart"), "{text}");
    }
}

#[test]
fn only_mermaid_and_one_pair_of_braces_make_a_diagram() {
    let mut hl = Highlighter::new();
    let fence = |info: &str| format!("```{info}\nflowchart TD\nA-->B\n```\n");
    for (info, drawn) in [("mermaid}", false), ("{{{mermaid", false), ("{mermaid", false), ("{{mermaid}}", false), ("{MerMaid}", true)] {
        let r = render(&fence(info), 40, Depth::None, &mut hl);
        assert_eq!(r.diagrams.len(), usize::from(drawn), "{info}");
        if !drawn {
            assert!(segs_text(&r.lines[0].segs).starts_with(&format!("╭─ {info} ─")), "{info}");
        }
    }
}

/// The plain lines of `source` laid out at `width`.
fn plain_lines(source: &str, width: usize, hl: &mut Highlighter) -> Vec<String> {
    let r = render(source, width, Depth::None, hl);
    for line in &r.lines {
        let w = segs_width(&line.segs);
        assert!(w <= width, "at width {width}: a line is {w} wide: {:?}", segs_text(&line.segs));
    }
    r.lines.iter().map(|l| segs_text(&l.segs)).collect()
}

#[test]
fn a_diagram_over_64_kib_gets_its_outline() {
    let mut hl = Highlighter::new();
    let comments = "%% a comment that only makes the source longer\n".repeat(1400);
    let source = format!("```mermaid\nflowchart TD\nA[Start] --> B[End]\n{comments}```\n");
    assert!(source.len() > 64 * 1024);
    let lines = plain_lines(&source, 60, &mut hl);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[0].starts_with("── mermaid · flowchart (outline) ──"), "{}", lines[0]);
    assert_eq!(lines[1], "Start ─▸ End");
    assert!(lines[2].ends_with("── too large to draw here ──"), "{}", lines[2]);

    let source = format!("```mermaid\nsequenceDiagram\nAlice->>Bob: Hi\n{comments}```\n");
    let lines = plain_lines(&source, 60, &mut hl);
    assert_eq!(lines[1..], ["1. Alice ─▸ Bob: Hi", &format!("{}── too large to draw here ──", "─".repeat(32))]);
}

#[test]
fn diagram_titles_are_cut_before_they_are_dropped() {
    let mut hl = Highlighter::new();
    let outline = "```mermaid\n---\ntitle: Auth\n---\nflowchart LR\nsubgraph G\nA --> B\nend\n```\n";
    let transcript = "```mermaid\n---\ntitle: Auth\n---\nsequenceDiagram\nAlice->>Bob: Hi\n```\n";
    let top = |source: &str, width: usize, hl: &mut Highlighter| plain_lines(source, width, hl)[0].clone();
    let rule = |label: &str, width: usize| format!("── {label} {}", "─".repeat(width - label.chars().count() - 4));

    assert_eq!(top(outline, 43, &mut hl), rule("mermaid · flowchart (outline) · Auth", 43));
    assert_eq!(top(outline, 41, &mut hl), rule("mermaid · flowchart · Auth", 41));
    assert_eq!(top(outline, 30, &mut hl), rule("mermaid · Auth", 30));
    assert_eq!(top(transcript, 44, &mut hl), rule("mermaid · sequence (transcript) · Auth", 44));
    assert_eq!(top(transcript, 43, &mut hl), rule("mermaid · sequence · Auth", 43));
    assert_eq!(top(transcript, 19, &mut hl), rule("mermaid · Au…", 19));
    assert_eq!(top(transcript, 18, &mut hl), rule("mermaid · A…", 18));
    assert_eq!(top(transcript, 17, &mut hl), rule("mermaid", 17));
    for width in 18..=120 {
        for source in [outline, transcript] {
            assert!(top(source, width, &mut hl).contains(" · A"), "no title at width {width}");
        }
    }

    // Search finds the title in the rule, where it is shown, even when cut.
    for width in [44, 30, 19] {
        let r = render(transcript, width, Depth::None, &mut hl);
        let found = search::find(&r, "au");
        assert_eq!(found.len(), 1, "at width {width}");
        let (line, col, w) = found[0].spans[0];
        let shown: String = segs_text(&r.lines[line].segs).chars().skip(col).take(w).collect();
        assert_eq!((line, shown.as_str()), (0, "Au"), "at width {width}");
    }
    assert!(search::find(&render(transcript, 17, Depth::None, &mut hl), "au").is_empty());

    // A long title is cut, keeping "(outline)" while 10 columns are left.
    let long = outline.replace("Auth", "Signing in with a password and a second factor");
    assert_eq!(top(&long, 50, &mut hl), rule("mermaid · flowchart (outline) · Signing in …", 50));
    assert_eq!(top(&long, 47, &mut hl), rule("mermaid · flowchart · Signing in with a …", 47));
}

#[test]
fn search_finds_diagram_titles() {
    let mut hl = Highlighter::new();
    let source = "> ```mermaid\n> sequenceDiagram\n>   title Login flow\n>   Alice->>Bob: Hello Bob\n>   Bob-->>Alice: Hi Alice\n> ```\n\n\
                  ```mermaid\n---\ntitle: Release pipeline\n---\nflowchart TD\n  A --> B\n```\n";
    let r = render(source, 70, Depth::None, &mut hl);
    for query in ["Login flow", "Release pipeline", "Hello Bob"] {
        let found = search::find(&r, query);
        assert_eq!(found.len(), 1, "{query}");
        let (line, col, w) = found[0].spans[0];
        let shown: String = segs_text(&r.lines[line].segs).chars().skip(col).take(w).collect();
        assert_eq!(shown, query);
    }
}

#[test]
fn diagrams_fit_every_width() {
    let mut hl = Highlighter::new();
    let sources = [
        "```mermaid\n---\ntitle: A rather long title for a diagram\n---\nflowchart RL\nA[Start here] --> B{Check it} -->|yes| C@{ shape: bolt }\n```\n",
        "```mermaid\nflowchart LR\nsubgraph G [Group one]\nA --> B\nend\n```\n",
        "```mermaid\n---\ntitle: Auth\n---\nsequenceDiagram\nAlice->>+Bob: Hello there\nalt ok\nBob-->>-Alice: Fine\nend\n```\n",
        "```mermaid\nflowchart TD\nA --> \n```\n",
    ];
    for source in sources {
        for width in 1..=120 {
            plain_lines(source, width, &mut hl);
        }
    }
}

#[test]
fn bottom_rules_note_direction_changes_and_simplified_shapes() {
    let mut hl = Highlighter::new();
    let last = |source: &str, width: usize, hl: &mut Highlighter| plain_lines(source, width, hl).last().cloned().unwrap();
    let rl = "```mermaid\nflowchart RL\nA[First step] --> B[Second step] --> C[Third step] --> D[Fourth step]\n```\n";
    assert!(last(rl, 120, &mut hl).ends_with(" drawn left to right ──"));
    assert!(last(rl, 30, &mut hl).ends_with(" drawn top-down to fit ──"));
    let bt = "```mermaid\nflowchart BT\nA@{ shape: bolt } --> B\n```\n";
    assert!(last(bt, 80, &mut hl).ends_with(" drawn top-down; simplified ──"));
    let td = "```mermaid\nflowchart TD\nA@{ shape: hex } --> B\n```\n";
    assert_eq!(last(td, 80, &mut hl), "─".repeat(80));
}

#[test]
fn source_mode_labels_a_fence_by_its_first_word() {
    use folio::diagram::Mode;
    use folio::layout::TargetKind;
    let mut hl = Highlighter::new();
    let theme = Theme::new(true, Depth::None);
    let doc = doc::parse("```mermaid\tfoo\nflowchart LR\nA --> B\n```\n");
    let r = layout::layout(&doc, 40, &theme, &mut hl, &mut Diagrams::new(Mode::Source));
    assert!(segs_text(&r.lines[0].segs).starts_with("╭─ mermaid ─"), "{}", segs_text(&r.lines[0].segs));
    assert!(matches!(&r.targets[0].kind, TargetKind::Code { lang, .. } if lang == "mermaid"));

    // The source shown in place of a drawing has the same label.
    let r = render("```mermaid\tfoo\nflowchart LR\nA --> B\n", 40, Depth::None, &mut hl);
    assert!(segs_text(&r.lines[0].segs).starts_with("╭─ mermaid ─"), "{}", segs_text(&r.lines[0].segs));
}

#[test]
fn diagram_roles_get_their_theme_styles() {
    use folio::text::Seg;
    let mut hl = Highlighter::new();
    let theme = Theme::new(true, Depth::TrueColor);
    let source = "```mermaid\nflowchart TD\n    A[Start] -->|yes| B[Done]\n```\n";
    let r = layout::layout(&doc::parse(source), 40, &theme, &mut hl, &mut Diagrams::default());
    let segs: Vec<&Seg> = r.lines.iter().flat_map(|l| &l.segs).collect();
    let style_of = |text: &str| segs.iter().find(|s| s.text.contains(text)).map(|s| s.style).unwrap_or_else(|| panic!("no {text:?}"));
    assert_eq!(style_of("┌"), theme.diagram_line, "box border");
    assert_eq!(style_of("Start"), ratatui::style::Style::new(), "node text");
    assert_eq!(style_of("yes"), theme.diagram_label, "edge label");
    assert_eq!(style_of("▾"), ratatui::style::Style::new(), "arrowhead");
    assert_eq!(style_of("mermaid"), theme.code_label, "frame title");
    // An edge line between the label and the arrowhead.
    let edge = r.lines.iter().flat_map(|l| &l.segs).find(|s| s.text.trim() == "│" && s.style != theme.diagram_line);
    assert_eq!(edge.map(|s| s.style), Some(theme.diagram_edge), "edge line");
    // Search finds a drawn label where it is drawn.
    let found = search::find(&r, "Done");
    assert_eq!(found.len(), 1);
}
