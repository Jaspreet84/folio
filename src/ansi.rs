//! Printing rendered lines with ANSI escape codes (or as plain text).

use std::fmt::Write;

use ratatui::style::{Color, Modifier, Style};

use crate::layout::Rendered;
use crate::text::segs_text;

pub fn render(r: &Rendered, color: bool) -> String {
    let mut out = String::new();
    for line in &r.lines {
        if !color {
            out.push_str(segs_text(&line.segs).trim_end());
            out.push('\n');
            continue;
        }
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
        if open {
            out.push_str("\x1b[0m");
        }
        out.push('\n');
    }
    out
}

fn sgr(style: Style) -> String {
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
}
