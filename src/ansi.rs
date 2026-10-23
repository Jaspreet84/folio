//! Printing rendered lines with ANSI escape codes (or as plain text).

use std::fmt::Write;

use ratatui::style::{Color, Modifier, Style};

use crate::layout::Rendered;
use crate::text::segs_text;

/// Render lines as text. With `color`, styles become SGR codes, and each
/// link whose entry in `link_urls` is set becomes an OSC 8 hyperlink.
pub fn render(r: &Rendered, color: bool, link_urls: &[Option<String>]) -> String {
    let mut out = String::new();
    for line in &r.lines {
        if !color {
            out.push_str(segs_text(&line.segs).trim_end());
            out.push('\n');
            continue;
        }
        let mut link: Option<&str> = None;
        let mut open = false;
        let mut segs = line.segs.as_slice();
        // Drop trailing unstyled whitespace (list indents on blank lines).
        while let Some((last, rest)) = segs.split_last() {
            if last.style == Style::new() && last.text.trim().is_empty() {
                segs = rest;
            } else {
                break;
            }
        }
        for seg in segs {
            let url = seg.link.and_then(|id| link_urls.get(id as usize)?.as_deref());
            if url != link {
                if link.is_some() {
                    out.push_str(OSC8_CLOSE);
                }
                if let Some(u) = url {
                    out.push_str(&osc8_open(u));
                }
                link = url;
            }
            let codes = sgr(seg.style);
            if open {
                out.push_str("\x1b[0m");
                open = false;
            }
            if !codes.is_empty() {
                let _ = write!(out, "\x1b[{codes}m");
                open = true;
            }
            out.push_str(&seg.text);
        }
        if link.is_some() {
            out.push_str(OSC8_CLOSE);
        }
        if open {
            out.push_str("\x1b[0m");
        }
        out.push('\n');
    }
    out
}

pub const OSC8_CLOSE: &str = "\x1b]8;;\x1b\\";

pub fn osc8_open(url: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\")
}

/// SGR parameters for a style, e.g. "1;38;2;97;175;239".
pub fn sgr(style: Style) -> String {
    let mut codes: Vec<String> = Vec::new();
    let m = style.add_modifier;
    for (flag, code) in [
        (Modifier::BOLD, "1"),
        (Modifier::DIM, "2"),
        (Modifier::ITALIC, "3"),
        (Modifier::UNDERLINED, "4"),
        (Modifier::REVERSED, "7"),
        (Modifier::CROSSED_OUT, "9"),
    ] {
        if m.contains(flag) {
            codes.push(code.into());
        }
    }
    if let Some(c) = style.fg.and_then(|c| color_code(c, 30)) {
        codes.push(c);
    }
    if let Some(c) = style.bg.and_then(|c| color_code(c, 40)) {
        codes.push(c);
    }
    codes.join(";")
}

fn color_code(c: Color, base: u8) -> Option<String> {
    let named = |n: u8| Some((base + n).to_string());
    let bright = |n: u8| Some((base + 60 + n).to_string());
    match c {
        Color::Reset => None,
        Color::Black => named(0),
        Color::Red => named(1),
        Color::Green => named(2),
        Color::Yellow => named(3),
        Color::Blue => named(4),
        Color::Magenta => named(5),
        Color::Cyan => named(6),
        Color::Gray => named(7),
        Color::DarkGray => bright(0),
        Color::LightRed => bright(1),
        Color::LightGreen => bright(2),
        Color::LightYellow => bright(3),
        Color::LightBlue => bright(4),
        Color::LightMagenta => bright(5),
        Color::LightCyan => bright(6),
        Color::White => bright(7),
        Color::Indexed(i) => Some(format!("{};5;{i}", base + 8)),
        Color::Rgb(r, g, b) => Some(format!("{};2;{r};{g};{b}", base + 8)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_sgr_codes() {
        let s = Style::new().fg(Color::Rgb(1, 2, 3)).add_modifier(Modifier::BOLD);
        assert_eq!(sgr(s), "1;38;2;1;2;3");
        assert_eq!(sgr(Style::new().bg(Color::Indexed(236))), "48;5;236");
        assert_eq!(sgr(Style::new().fg(Color::Reset)), "");
    }

    #[test]
    fn wraps_links_in_osc8() {
        use crate::layout::{RLine, Rendered};
        use crate::text::Seg;
        let r = Rendered {
            lines: vec![RLine {
                segs: vec![Seg::plain("see "), Seg::plain("docs").with_link(Some(0)), Seg::plain(".")],
                ..RLine::default()
            }],
            ..Rendered::default()
        };
        let urls = [Some("https://example.com".to_string())];
        assert_eq!(render(&r, true, &urls), "see \x1b]8;;https://example.com\x1b\\docs\x1b]8;;\x1b\\.\n");
        assert_eq!(render(&r, true, &[]), "see docs.\n");
        assert_eq!(render(&r, false, &urls), "see docs.\n");
    }
}
