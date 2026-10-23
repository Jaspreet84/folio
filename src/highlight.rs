//! Syntax highlighting for code blocks, with the syntax set loaded lazily and
//! results cached so a resize doesn't highlight everything again.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use ratatui::style::{Modifier, Style};
use syntect::easy::HighlightLines;
use syntect::highlighting::FontStyle;
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::text::Seg;
use crate::theme::Theme;

#[derive(Default)]
pub struct Highlighter {
    syntaxes: OnceCell<SyntaxSet>,
    themes: OnceCell<two_face::theme::EmbeddedLazyThemeSet>,
    cache: HashMap<u64, Option<Vec<Vec<Seg>>>>,
}

impl Highlighter {
    pub fn new() -> Highlighter {
        Highlighter::default()
    }

    /// Highlight `code` as `lang`. Returns one list of segments per line, or
    /// `None` when colors are off or the language is unknown.
    pub fn highlight(&mut self, lang: &str, code: &str, theme: &Theme) -> Option<Vec<Vec<Seg>>> {
        if !theme.has_color() || lang.is_empty() {
            return None;
        }
        let mut hasher = DefaultHasher::new();
        (lang, code, theme.dark, theme.depth as u8).hash(&mut hasher);
        let key = hasher.finish();
        if let Some(hit) = self.cache.get(&key) {
            return hit.clone();
        }
        let result = self.run(lang, code, theme);
        self.cache.insert(key, result.clone());
        result
    }

    fn run(&self, lang: &str, code: &str, theme: &Theme) -> Option<Vec<Vec<Seg>>> {
        let syntaxes = self.syntaxes.get_or_init(two_face::syntax::extra_newlines);
        let syntax = find_syntax(syntaxes, lang)?;
        let themes = self.themes.get_or_init(two_face::theme::extra);
        let syn_theme = themes.get(theme.syntax_theme());
        let default_fg = syn_theme.settings.foreground;
        let mut h = HighlightLines::new(syntax, syn_theme);
        let mut lines = Vec::new();
        for line in LinesWithEndings::from(code) {
            let ranges = h.highlight_line(line, syntaxes).ok()?;
            let mut segs: Vec<Seg> = Vec::new();
            for (st, text) in ranges {
                let text = text.trim_end_matches(['\n', '\r']);
                if text.is_empty() {
                    continue;
                }
                let mut style = Style::new();
                // The theme's plain-text color is left to the terminal, so
                // code stays readable even if light/dark detection was wrong.
                if Some(st.foreground) != default_fg {
                    let c = st.foreground;
                    style = style.fg(theme.rgb(c.r, c.g, c.b));
                }
                if st.font_style.contains(FontStyle::BOLD) {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if st.font_style.contains(FontStyle::ITALIC) {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                match segs.last_mut() {
                    Some(last) if last.style == style => last.text.push_str(text),
                    _ => segs.push(Seg::new(text, style)),
                }
            }
            lines.push(segs);
        }
        if code.ends_with('\n') || code.is_empty() {
            lines.push(Vec::new());
        }
        Some(lines)
    }
}

fn find_syntax<'s>(set: &'s SyntaxSet, lang: &str) -> Option<&'s SyntaxReference> {
    let lower = lang.to_ascii_lowercase();
    let token = match lower.as_str() {
        "text" | "txt" | "plain" | "plaintext" | "ascii" | "output" => return None,
        "sh" | "shell" | "zsh" | "console" | "terminal" | "shellsession" | "shell-session" => "bash",
        "yml" => "yaml",
        "py" | "python3" => "python",
        "golang" => "go",
        "c++" => "cpp",
        "jsonc" | "json5" => "json",
        "ps1" | "powershell" | "pwsh" => "ps1",
        "dockerfile" | "docker" => "Dockerfile",
        "md" | "markdown" => "md",
        "kt" => "kotlin",
        other => other,
    };
    set.find_syntax_by_token(token).or_else(|| set.find_syntax_by_name(lang))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Depth;

    #[test]
    fn highlights_known_languages() {
        let theme = Theme::new(true, Depth::TrueColor);
        let mut hl = Highlighter::new();
        let lines = hl.highlight("rust", "fn main() {}\nlet x = 1;", &theme).unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].len() > 1, "expected several styled runs");
        for lang in ["sh", "yml", "ts", "toml", "Dockerfile", "go", "py"] {
            assert!(hl.highlight(lang, "x", &theme).is_some(), "{lang} not found");
        }
        assert!(hl.highlight("no-such-language", "x", &theme).is_none());
    }

    #[test]
    fn skips_highlighting_without_color() {
        let theme = Theme::new(true, Depth::None);
        assert!(Highlighter::new().highlight("rust", "fn main() {}", &theme).is_none());
    }
}
