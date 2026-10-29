//! Colors and text styles for the light and dark themes.

use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    /// No colors at all (`NO_COLOR`, `--color never`). Bold and italic remain.
    None,
    Ansi256,
    TrueColor,
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub dark: bool,
    pub depth: Depth,
    /// The terminal supports OSC 8 hyperlinks, so URLs needn't be shown inline.
    pub hyperlinks: bool,
    pub heading: [Style; 6],
    pub heading_rule: [Style; 2],
    pub link: Style,
    pub url: Style,
    pub code_inline: Style,
    pub code_border: Style,
    pub code_label: Style,
    pub wrap_marker: Style,
    pub quote_bar: Style,
    pub alert: [Style; 5],
    pub list_marker: Style,
    pub task_done: Style,
    pub task_todo: Style,
    pub table_border: Style,
    pub table_header: Style,
    pub rule: Style,
    pub image: Style,
    pub footnote: Style,
    pub math: Style,
    pub dim: Style,
    pub bar: Style,
    pub bar_accent: Style,
    pub search_match: Style,
    pub search_current: Style,
    pub toc_current: Style,
    pub toc_selected: Style,
    /// The focused link.
    pub focus: Style,
    /// The border of a focused code block.
    pub focus_border: Style,
    /// Box borders and frames in Mermaid diagrams.
    pub diagram_line: Style,
    /// Edges in Mermaid diagrams.
    pub diagram_edge: Style,
    /// Edge labels in Mermaid diagrams.
    pub diagram_label: Style,
    /// The note under a diagram that was not drawn.
    pub diagram_note: Style,
}

struct Palette {
    h: [u32; 6],
    link: u32,
    muted: u32,
    code_fg: u32,
    code_bg: u32,
    border: u32,
    alert: [u32; 5],
    green: u32,
    bar_fg: u32,
    bar_bg: u32,
    match_bg: u32,
    current_bg: u32,
    match_fg: u32,
    selected_bg: u32,
    /// Diagram lines carry meaning, so they need more contrast than `border`.
    diagram_line: u32,
    diagram_edge: u32,
}

const DARK: Palette = Palette {
    h: [0x61AFEF, 0xC678DD, 0xE5C07B, 0x98C379, 0x56B6C2, 0x9DA5B4],
    link: 0x61AFEF,
    muted: 0x7F848E,
    code_fg: 0xE5A07B,
    code_bg: 0x2C313A,
    border: 0x4B5263,
    alert: [0x61AFEF, 0x98C379, 0xC678DD, 0xE5C07B, 0xE06C75],
    green: 0x98C379,
    bar_fg: 0xABB2BF,
    bar_bg: 0x2C313A,
    match_bg: 0x7A6A2E,
    current_bg: 0xE5C07B,
    match_fg: 0x1E2127,
    selected_bg: 0x3E4451,
    diagram_line: 0x9DA5B4,
    diagram_edge: 0x7F848E,
};

const LIGHT: Palette = Palette {
    h: [0x0550AE, 0x8250DF, 0x9A6700, 0x1A7F37, 0x0E7490, 0x57606A],
    link: 0x0969DA,
    muted: 0x6E7781,
    code_fg: 0xA3361F,
    code_bg: 0xEFF1F3,
    border: 0xC4CBD3,
    alert: [0x0969DA, 0x1A7F37, 0x8250DF, 0x9A6700, 0xCF222E],
    green: 0x1A7F37,
    bar_fg: 0x24292F,
    bar_bg: 0xEAEEF2,
    match_bg: 0xFFF1A8,
    current_bg: 0xF5A524,
    match_fg: 0x1F2328,
    selected_bg: 0xDDF4FF,
    diagram_line: 0x57606A,
    diagram_edge: 0x6E7781,
};

impl Theme {
    pub fn new(dark: bool, depth: Depth) -> Theme {
        let p = if dark { &DARK } else { &LIGHT };
        let fg = |rgb: u32| Style::new().fg(color(rgb, depth));
        let bold = Modifier::BOLD;
        let mut heading = [Style::new(); 6];
        for (i, h) in heading.iter_mut().enumerate() {
            *h = fg(p.h[i]).add_modifier(bold);
        }
        heading[5] = heading[5].add_modifier(Modifier::ITALIC);
        let mut alert = [Style::new(); 5];
        for (i, a) in alert.iter_mut().enumerate() {
            *a = fg(p.alert[i]);
        }
        let colorless = depth == Depth::None;
        Theme {
            dark,
            depth,
            hyperlinks: false,
            heading,
            heading_rule: [fg(p.h[0]), fg(p.border)],
            link: fg(p.link).add_modifier(Modifier::UNDERLINED),
            url: fg(p.muted),
            code_inline: fg(p.code_fg).bg(color(p.code_bg, depth)),
            code_border: fg(p.border),
            code_label: fg(p.muted).add_modifier(Modifier::ITALIC),
            wrap_marker: fg(p.muted),
            quote_bar: fg(p.border),
            alert,
            list_marker: fg(p.h[0]),
            task_done: fg(p.green),
            task_todo: fg(p.muted),
            table_border: fg(p.border),
            table_header: Style::new().add_modifier(bold),
            rule: fg(p.border),
            image: fg(p.muted).add_modifier(Modifier::ITALIC),
            footnote: fg(p.link),
            math: fg(p.h[4]).add_modifier(Modifier::ITALIC),
            dim: fg(p.muted),
            bar: if colorless {
                Style::new().add_modifier(Modifier::REVERSED)
            } else {
                fg(p.bar_fg).bg(color(p.bar_bg, depth))
            },
            bar_accent: if colorless {
                Style::new().add_modifier(Modifier::REVERSED | bold)
            } else {
                fg(p.h[0]).bg(color(p.bar_bg, depth)).add_modifier(bold)
            },
            search_match: if colorless {
                Style::new().add_modifier(Modifier::UNDERLINED)
            } else {
                fg(p.match_fg).bg(color(p.match_bg, depth))
            },
            search_current: if colorless {
                Style::new().add_modifier(Modifier::REVERSED)
            } else {
                fg(p.match_fg).bg(color(p.current_bg, depth)).add_modifier(bold)
            },
            toc_current: fg(p.h[0]).add_modifier(bold),
            toc_selected: if colorless {
                Style::new().add_modifier(Modifier::REVERSED)
            } else {
                Style::new().bg(color(p.selected_bg, depth))
            },
            focus: if colorless {
                Style::new().add_modifier(Modifier::REVERSED)
            } else {
                Style::new().bg(color(p.selected_bg, depth)).add_modifier(bold)
            },
            focus_border: if colorless { Style::new().add_modifier(bold) } else { fg(p.current_bg).add_modifier(bold) },
            diagram_line: fg(p.diagram_line),
            diagram_edge: fg(p.diagram_edge),
            diagram_label: fg(p.muted).add_modifier(Modifier::ITALIC),
            diagram_note: fg(p.muted).add_modifier(Modifier::ITALIC),
        }
    }

    pub fn has_color(&self) -> bool {
        self.depth != Depth::None
    }

    /// Convert a syntax-highlighting color for this terminal.
    pub fn rgb(&self, r: u8, g: u8, b: u8) -> Color {
        color(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b), self.depth)
    }

    pub fn syntax_theme(&self) -> two_face::theme::EmbeddedThemeName {
        use two_face::theme::EmbeddedThemeName::*;
        if self.dark { OneHalfDark } else { OneHalfLight }
    }
}

pub fn color(rgb: u32, depth: Depth) -> Color {
    let (r, g, b) = ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
    match depth {
        Depth::None => Color::Reset,
        Depth::TrueColor => Color::Rgb(r, g, b),
        Depth::Ansi256 => Color::Indexed(nearest_256(r, g, b)),
    }
}

/// Nearest color in the xterm 256-color palette (6x6x6 cube plus grays).
pub fn nearest_256(r: u8, g: u8, b: u8) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let level = |v: u8| -> usize {
        LEVELS
            .iter()
            .enumerate()
            .min_by_key(|(_, l)| (i32::from(**l) - i32::from(v)).abs())
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    let dist = |a: (u8, u8, u8)| -> i32 {
        let d = |x: u8, y: u8| (i32::from(x) - i32::from(y)).pow(2);
        d(a.0, r) + d(a.1, g) + d(a.2, b)
    };
    let (ri, gi, bi) = (level(r), level(g), level(b));
    let cube = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    let cube_index = 16 + 36 * ri + 6 * gi + bi;
    let avg = (u32::from(r) + u32::from(g) + u32::from(b)) / 3;
    let gray_step = ((avg.saturating_sub(8)) / 10).min(23) as u8;
    let gray_value = 8 + 10 * gray_step;
    let gray = (gray_value, gray_value, gray_value);
    if dist(gray) < dist(cube) { 232 + gray_step } else { cube_index as u8 }
}

/// Work out the color depth from the environment.
pub fn detect_depth() -> Depth {
    let env = |k: &str| std::env::var(k).unwrap_or_default().to_ascii_lowercase();
    let colorterm = env("COLORTERM");
    if colorterm.contains("truecolor") || colorterm.contains("24bit") {
        return Depth::TrueColor;
    }
    let term = env("TERM");
    if term.contains("truecolor") || term.contains("24bit") || term.contains("direct") {
        return Depth::TrueColor;
    }
    let program = env("TERM_PROGRAM");
    if ["iterm.app", "wezterm", "ghostty", "vscode", "kitty", "rio"].contains(&program.as_str())
        || term.contains("kitty")
        || term.contains("ghostty")
    {
        return Depth::TrueColor;
    }
    Depth::Ansi256
}

/// Whether the terminal is likely to support OSC 8 hyperlinks. Terminals
/// that don't simply show the text, so this only decides whether URLs are
/// also printed inline.
pub fn detect_hyperlinks() -> bool {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let term = env("TERM");
    let gnu_screen = term.starts_with("screen") && std::env::var_os("TMUX").is_none();
    !(env("TERM_PROGRAM") == "Apple_Terminal" || term == "linux" || term == "dumb" || gnu_screen)
}

/// Ask the terminal whether its background is dark. Falls back to
/// `COLORFGBG`, then to dark.
pub fn detect_dark() -> bool {
    use terminal_colorsaurus::{QueryOptions, ThemeMode, theme_mode};
    let mut options = QueryOptions::default();
    options.timeout = std::time::Duration::from_millis(400);
    if let Ok(mode) = theme_mode(options) {
        return mode == ThemeMode::Dark;
    }
    if let Ok(v) = std::env::var("COLORFGBG")
        && let Some(bg) = v.rsplit(';').next().and_then(|b| b.parse::<u8>().ok()) {
            return !(bg == 7 || bg >= 9);
        }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_to_256_colors() {
        assert_eq!(nearest_256(0, 0, 0), 16);
        assert_eq!(nearest_256(255, 255, 255), 231);
        assert_eq!(nearest_256(255, 0, 0), 196);
        assert_eq!(nearest_256(128, 128, 128), 244);
    }
}
