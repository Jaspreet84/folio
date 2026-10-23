//! The file picker: the Markdown files under a folder, with a filter.

use std::path::{Path, PathBuf};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::links::is_markdown;
use crate::text::{truncate, width};
use crate::theme::Theme;

/// Stop listing after this many files.
const MAX_FILES: usize = 5000;
/// Read titles only when there are at most this many files.
const MAX_TITLES: usize = 2000;

pub struct Entry {
    /// Path relative to the root, with forward slashes.
    pub rel: String,
    pub path: PathBuf,
    /// The document's first heading, if it has one.
    pub title: Option<String>,
}

pub struct Picker {
    pub root: PathBuf,
    pub entries: Vec<Entry>,
    truncated: bool,
    /// Index into the visible (filtered) entries.
    selected: usize,
    scroll: usize,
    filter: Option<String>,
}

pub enum Action {
    None,
    Open(PathBuf),
    Back,
    Help,
    Quit,
}

impl Picker {
    /// List the Markdown files under `root`, skipping hidden files and
    /// anything `.gitignore` excludes.
    pub fn scan(root: &Path) -> Picker {
        let mut entries = Vec::new();
        let mut truncated = false;
        let walker = ignore::WalkBuilder::new(root).hidden(true).git_ignore(true).max_depth(Some(12)).build();
        for entry in walker.flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) || !is_markdown(entry.path()) {
                continue;
            }
            if entries.len() >= MAX_FILES {
                truncated = true;
                break;
            }
            let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
            let rel = rel.to_string_lossy().replace('\\', "/");
            entries.push(Entry { rel, path: entry.path().to_path_buf(), title: None });
        }
        entries.sort_by_key(|e| sort_key(&e.rel));
        if entries.len() <= MAX_TITLES {
            for e in &mut entries {
                e.title = read_title(&e.path);
            }
        }
        Picker { root: root.to_path_buf(), entries, truncated, selected: 0, scroll: 0, filter: None }
    }

    fn visible(&self) -> Vec<usize> {
        match self.filter.as_deref().map(str::trim) {
            Some(f) if !f.is_empty() => (0..self.entries.len())
                .filter(|&i| {
                    let e = &self.entries[i];
                    fuzzy_match(f, &e.rel) || e.title.as_deref().is_some_and(|t| fuzzy_match(f, t))
                })
                .collect(),
            _ => (0..self.entries.len()).collect(),
        }
    }

    /// Select the entry for `path`, if it is listed.
    pub fn select_path(&mut self, path: &Path) {
        let visible = self.visible();
        if let Some(i) = visible.iter().position(|&i| same_file(&self.entries[i].path, path)) {
            self.selected = i;
        }
    }

    pub fn key(&mut self, k: KeyEvent, page: usize) -> Action {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let last = self.visible().len().saturating_sub(1);
        let sel = self.selected;
        let open = |p: &Picker| {
            p.visible().get(p.selected).map_or(Action::None, |&i| Action::Open(p.entries[i].path.clone()))
        };
        if let Some(filter) = self.filter.as_mut() {
            match k.code {
                KeyCode::Esc => self.filter = None,
                KeyCode::Backspace if filter.is_empty() => self.filter = None,
                KeyCode::Backspace => {
                    filter.pop();
                    self.selected = 0;
                }
                KeyCode::Char('u') if ctrl => {
                    filter.clear();
                    self.selected = 0;
                }
                KeyCode::Enter => return open(self),
                KeyCode::Up => self.selected = sel.saturating_sub(1),
                KeyCode::Char('p' | 'k') if ctrl => self.selected = sel.saturating_sub(1),
                KeyCode::Down => self.selected = (sel + 1).min(last),
                KeyCode::Char('n' | 'j') if ctrl => self.selected = (sel + 1).min(last),
                KeyCode::Char(c) if !ctrl => {
                    filter.push(c);
                    self.selected = 0;
                }
                _ => {}
            }
            return Action::None;
        }
        match k.code {
            KeyCode::Char('c') if ctrl => return Action::Quit,
            KeyCode::Char('j') | KeyCode::Down => self.selected = (sel + 1).min(last),
            KeyCode::Char('k') | KeyCode::Up => self.selected = sel.saturating_sub(1),
            KeyCode::Char('g') | KeyCode::Home => self.selected = 0,
            KeyCode::Char('G') | KeyCode::End => self.selected = last,
            KeyCode::Char(' ') | KeyCode::PageDown => self.selected = (sel + page).min(last),
            KeyCode::Char('b') | KeyCode::PageUp => self.selected = sel.saturating_sub(page),
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => return open(self),
            KeyCode::Char('/') => {
                self.filter = Some(String::new());
                self.selected = 0;
            }
            KeyCode::Char('p') if ctrl => {
                self.filter = Some(String::new());
                self.selected = 0;
            }
            KeyCode::Char('H') | KeyCode::Backspace | KeyCode::Left | KeyCode::Esc => return Action::Back,
            KeyCode::Char('o') if ctrl => return Action::Back,
            KeyCode::Char('?') | KeyCode::F(1) => return Action::Help,
            KeyCode::Char('q') => return Action::Quit,
            _ => {}
        }
        Action::None
    }

    /// Draw the picker over `area`. Returns where the cursor goes.
    pub fn draw(&mut self, buf: &mut Buffer, area: Rect, t: &Theme, message: Option<&str>) -> Option<(u16, u16)> {
        if area.height < 3 || area.width < 10 {
            return None;
        }
        let w = usize::from(area.width);
        // Header.
        let header = Rect::new(area.x, area.y, area.width, 1);
        buf.set_style(header, t.bar);
        let count = format!(
            " {}{} Markdown file{} ",
            self.entries.len(),
            if self.truncated { "+" } else { "" },
            if self.entries.len() == 1 { "" } else { "s" }
        );
        let room = w.saturating_sub(width(&count) + 1);
        let title = truncate(&format!(" folio  ·  {}", self.root.display()), room);
        buf.set_string(area.x, area.y, &title, t.bar_accent);
        if width(&count) + 4 < w {
            buf.set_string(area.x + (w - width(&count)) as u16, area.y, &count, t.bar);
        }
        // Filter line.
        let mut y = area.y + 1;
        let mut cursor = None;
        if let Some(filter) = &self.filter {
            let shown = truncate(&format!(" › {filter}"), w.saturating_sub(2));
            buf.set_string(area.x, y, &shown, Style::new());
            cursor = Some((area.x + width(&shown) as u16, y));
            y += 1;
        }
        // List.
        let list_h = usize::from(area.bottom().saturating_sub(1).saturating_sub(y));
        let visible = self.visible();
        self.selected = self.selected.min(visible.len().saturating_sub(1));
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + list_h {
            self.scroll = self.selected + 1 - list_h;
        }
        if visible.is_empty() {
            let msg = if self.entries.is_empty() { "No Markdown files here" } else { "No matches" };
            buf.set_string(area.x + 2, y, truncate(msg, w.saturating_sub(2)), t.dim);
        }
        for (row, (vi, &i)) in visible.iter().enumerate().skip(self.scroll).take(list_h).enumerate() {
            let yy = y + row as u16;
            let e = &self.entries[i];
            let selected = vi == self.selected;
            let (dir, name) = match e.rel.rfind('/') {
                Some(p) => (&e.rel[..=p], &e.rel[p + 1..]),
                None => ("", e.rel.as_str()),
            };
            let marker = if selected { " ▸ " } else { "   " };
            let mut spans = vec![
                Span::styled(marker, t.toc_current),
                Span::styled(dir.to_string(), t.dim),
                Span::styled(name.to_string(), Style::new().add_modifier(Modifier::BOLD)),
            ];
            let used = 3 + width(&e.rel);
            if let Some(title) = &e.title {
                let room = w.saturating_sub(used + 4);
                if room > 8 {
                    spans.push(Span::styled(format!("   {}", truncate(title, room)), t.dim));
                }
            }
            if selected {
                buf.set_style(Rect::new(area.x, yy, area.width, 1), t.toc_selected);
            }
            buf.set_line(area.x, yy, &Line::from(spans), area.width);
        }
        // Footer.
        let footer = Rect::new(area.x, area.bottom() - 1, area.width, 1);
        buf.set_style(footer, t.bar);
        if let Some(msg) = message {
            buf.set_stringn(footer.x + 1, footer.y, msg, w.saturating_sub(2), t.bar_accent);
            return cursor;
        }
        let hints: &[(&str, &str)] = if self.filter.is_some() {
            &[("type", "to filter"), ("↑↓", "select"), ("Enter", "open"), ("Esc", "clear")]
        } else {
            &[("↑↓", "move"), ("Enter", "open"), ("/", "filter"), ("H", "back"), ("?", "help"), ("q", "quit")]
        };
        let key = t.bar.add_modifier(Modifier::BOLD);
        let mut spans = vec![Span::raw(" ")];
        let mut used = 1;
        for (k, label) in hints {
            let need = width(k) + width(label) + 4;
            if used + need > w {
                break;
            }
            spans.push(Span::styled(*k, key));
            spans.push(Span::styled(format!(" {label}   "), t.bar));
            used += need;
        }
        buf.set_line(footer.x, footer.y, &Line::from(spans), footer.width);
        cursor
    }
}

/// Sort by folder, then README first, then by name, ignoring case.
fn sort_key(rel: &str) -> (String, bool, String) {
    let lower = rel.to_lowercase();
    let (dir, name) = match lower.rfind('/') {
        Some(p) => (lower[..p].to_string(), lower[p + 1..].to_string()),
        None => (String::new(), lower.clone()),
    };
    (dir, !name.starts_with("readme"), name)
}

/// The first heading of a Markdown file, from its first 8 KB.
fn read_title(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut buf = vec![0; 8192];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)).ok()?;
    let text = String::from_utf8_lossy(&buf[..n]);
    let mut lines = text.lines().peekable();
    if lines.peek().is_some_and(|l| l.trim() == "---") {
        lines.next();
        for l in lines.by_ref() {
            if l.trim() == "---" || l.trim() == "..." {
                break;
            }
        }
    }
    let mut prev: Option<&str> = None;
    for line in lines {
        let trimmed = line.trim();
        if let Some(h) = trimmed.strip_prefix("# ") {
            return clean_title(h.trim_end_matches('#'));
        }
        if let Some(p) = prev.filter(|p| !p.trim().is_empty())
            && !trimmed.is_empty() && trimmed.chars().all(|c| c == '=') {
                return clean_title(p);
            }
        prev = Some(line);
    }
    None
}

fn clean_title(s: &str) -> Option<String> {
    let t: String = crate::doc::clean(s.trim()).replace(['*', '`'], "");
    (!t.is_empty()).then(|| truncate(&t, 80))
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// Case-insensitive subsequence match, ignoring spaces in the query.
pub fn fuzzy_match(query: &str, text: &str) -> bool {
    let mut hay = text.chars().flat_map(char::to_lowercase);
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .all(|q| hay.any(|h| h == q))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_markdown_files_readme_first_and_reads_titles() {
        let dir = std::env::temp_dir().join(format!("folio-picker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        let files = [
            ("zeta.md", "# Zeta notes\n"),
            ("README.md", "---\ntitle: x\n---\n\n# Project\n"),
            ("docs/guide.md", "Guide\n=====\n"),
            ("docs/raw.txt", "# not markdown"),
            (".hidden/secret.md", "# hidden"),
        ];
        for (f, body) in files {
            std::fs::write(dir.join(f), body).unwrap();
        }
        let p = Picker::scan(&dir);
        let rels: Vec<&str> = p.entries.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(rels, vec!["README.md", "zeta.md", "docs/guide.md"]);
        let titles: Vec<Option<&str>> = p.entries.iter().map(|e| e.title.as_deref()).collect();
        assert_eq!(titles, vec![Some("Project"), Some("Zeta notes"), Some("Guide")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fuzzy_matches_subsequences() {
        assert!(fuzzy_match("dg", "docs/guide.md"));
        assert!(fuzzy_match("inst lin", "Install on Linux"));
        assert!(!fuzzy_match("xyz", "Install"));
    }
}
