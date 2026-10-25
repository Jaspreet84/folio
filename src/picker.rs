//! The file picker: the Markdown files under a folder, with a filter.
//!
//! The folder is scanned on a background thread, so the picker opens at
//! once and fills in as files are found. A big folder (a home folder, `/usr`)
//! can hold hundreds of thousands of files, and walking them all before
//! drawing anything made folio look frozen for seconds.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use ignore::DirEntry;
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
/// Read the titles of only this many files.
const MAX_TITLES: usize = 2000;
/// Don't descend deeper than this.
const MAX_DEPTH: usize = 12;

pub struct Entry {
    /// Path relative to the root, with forward slashes.
    pub rel: String,
    pub path: PathBuf,
    /// The document's first heading, if it has one.
    pub title: Option<String>,
    sort: (String, bool, String),
}

/// What the scanning thread sends.
enum Found {
    Entry(Entry),
    /// More than `MAX_FILES` files; the rest were skipped.
    Truncated,
}

pub struct Picker {
    pub root: PathBuf,
    /// Sorted by folder, then README first, then name.
    pub entries: Vec<Entry>,
    truncated: bool,
    /// Index into the visible (filtered) entries.
    selected: usize,
    scroll: usize,
    filter: Option<String>,
    /// Files still arriving from the scan; `None` once it has finished.
    incoming: Option<Receiver<Found>>,
    /// Tells the scanning thread to stop when the picker goes away.
    stop: Arc<AtomicBool>,
    /// The file the selection stays on while entries arrive: the file the
    /// reader came from, or the one they moved to. `None` keeps the
    /// selection at the top.
    follow: Option<PathBuf>,
}

pub enum Action {
    None,
    Open(PathBuf),
    Back,
    Help,
    Quit,
}

impl Drop for Picker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Picker {
    /// Start listing the Markdown files under `root` on a background thread.
    /// Call [`Picker::pull`] to take in what it has found so far.
    pub fn start(root: &Path) -> Picker {
        // A canonical root gives canonical entry paths, so finding a file in
        // the list is a plain comparison, with no file-system calls.
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (thread_root, thread_stop) = (root.clone(), Arc::clone(&stop));
        std::thread::spawn(move || scan(&thread_root, &tx, &thread_stop));
        Picker {
            root,
            entries: Vec::new(),
            truncated: false,
            selected: 0,
            scroll: 0,
            filter: None,
            incoming: Some(rx),
            stop,
            follow: None,
        }
    }

    /// Whether the scan is still running.
    pub fn scanning(&self) -> bool {
        self.incoming.is_some()
    }

    /// Take in the files found since the last call. Returns true if the list
    /// changed. The selection stays on the file it follows, if any, and
    /// otherwise at the top.
    pub fn pull(&mut self) -> bool {
        let Some(rx) = self.incoming.take() else { return false };
        let mut changed = false;
        loop {
            match rx.try_recv() {
                Ok(found) => {
                    self.add(found);
                    changed = true;
                }
                Err(TryRecvError::Empty) => {
                    self.incoming = Some(rx);
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    changed = true;
                    break;
                }
            }
        }
        if changed {
            self.reselect();
        }
        changed
    }

    /// Wait for the scan to finish.
    pub fn wait(&mut self) {
        if let Some(rx) = self.incoming.take() {
            for found in rx {
                self.add(found);
            }
            self.reselect();
        }
    }

    fn add(&mut self, found: Found) {
        match found {
            Found::Entry(e) => {
                let at = self.entries.partition_point(|x| x.sort < e.sort);
                self.entries.insert(at, e);
            }
            Found::Truncated => self.truncated = true,
        }
    }

    fn selected_path(&self) -> Option<PathBuf> {
        self.visible().get(self.selected).map(|&i| self.entries[i].path.clone())
    }

    /// Put the selection back on the followed file, if it is listed.
    fn reselect(&mut self) {
        let Some(path) = &self.follow else { return };
        if let Some(i) = self.visible().iter().position(|&i| self.entries[i].path == *path) {
            self.selected = i;
        }
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

    /// Select the entry for `path`, now or when the scan finds it, and keep
    /// it selected as other files arrive.
    pub fn select_path(&mut self, path: &Path) {
        self.follow = Some(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()));
        self.reselect();
    }

    fn open(&self) -> Action {
        self.selected_path().map_or(Action::None, Action::Open)
    }

    /// Close the filter, keeping the same file selected in the full list.
    fn leave_filter(&mut self) {
        if let Some(path) = self.selected_path() {
            self.follow = Some(path);
        }
        self.filter = None;
        self.reselect();
    }

    pub fn key(&mut self, k: KeyEvent, page: usize) -> Action {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let last = self.visible().len().saturating_sub(1);
        let sel = self.selected;
        // Moving makes the selection follow the new file; changing the filter
        // puts it at the top of the new list. Other keys leave it alone.
        let mut moved = false;
        let mut refiltered = false;
        if let Some(filter) = self.filter.as_mut() {
            match k.code {
                KeyCode::Esc => self.leave_filter(),
                KeyCode::Backspace if filter.is_empty() => self.leave_filter(),
                KeyCode::Backspace => {
                    filter.pop();
                    refiltered = true;
                }
                KeyCode::Char('u') if ctrl => {
                    filter.clear();
                    refiltered = true;
                }
                KeyCode::Enter => return self.open(),
                KeyCode::Up => {
                    self.selected = sel.saturating_sub(1);
                    moved = true;
                }
                KeyCode::Char('p' | 'k') if ctrl => {
                    self.selected = sel.saturating_sub(1);
                    moved = true;
                }
                KeyCode::Down => {
                    self.selected = (sel + 1).min(last);
                    moved = true;
                }
                KeyCode::Char('n' | 'j') if ctrl => {
                    self.selected = (sel + 1).min(last);
                    moved = true;
                }
                KeyCode::Char(c) if !ctrl => {
                    filter.push(c);
                    refiltered = true;
                }
                _ => {}
            }
        } else {
            let target = match k.code {
                KeyCode::Char('c') if ctrl => return Action::Quit,
                KeyCode::Char('j') | KeyCode::Down => Some((sel + 1).min(last)),
                KeyCode::Char('k') | KeyCode::Up => Some(sel.saturating_sub(1)),
                KeyCode::Char('g') | KeyCode::Home => Some(0),
                KeyCode::Char('G') | KeyCode::End => Some(last),
                KeyCode::Char(' ') | KeyCode::PageDown => Some((sel + page).min(last)),
                KeyCode::Char('b') | KeyCode::PageUp => Some(sel.saturating_sub(page)),
                KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => return self.open(),
                KeyCode::Char('/') => {
                    self.filter = Some(String::new());
                    refiltered = true;
                    None
                }
                KeyCode::Char('p') if ctrl => {
                    self.filter = Some(String::new());
                    refiltered = true;
                    None
                }
                KeyCode::Char('H') | KeyCode::Backspace | KeyCode::Left | KeyCode::Esc => return Action::Back,
                KeyCode::Char('o') if ctrl => return Action::Back,
                KeyCode::Char('?') | KeyCode::F(1) => return Action::Help,
                KeyCode::Char('q') => return Action::Quit,
                _ => None,
            };
            if let Some(i) = target {
                self.selected = i;
                moved = true;
            }
        }
        if moved {
            self.follow = self.selected_path();
        }
        if refiltered {
            self.selected = 0;
            self.follow = None;
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
            " {}{}{} Markdown file{} ",
            if self.scanning() { "Scanning… " } else { "" },
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
            let msg = match (self.entries.is_empty(), self.scanning()) {
                (true, true) => "Looking for Markdown files…",
                (true, false) => "No Markdown files here",
                (false, true) => "No matches yet…",
                (false, false) => "No matches",
            };
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

/// Walk `root` and send each Markdown file found, until done, until
/// `MAX_FILES`, or until the picker is dropped.
fn scan(root: &Path, tx: &Sender<Found>, stop: &AtomicBool) {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .max_depth(Some(MAX_DEPTH))
        // Don't wander into other drives, network mounts, /proc or WSL's /mnt/c.
        .same_file_system(true)
        .filter_entry(move |e| !skip_dir(e, home.as_deref()))
        .build();
    let mut count = 0;
    for entry in walker {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) || !is_markdown(entry.path()) {
            continue;
        }
        if count == MAX_FILES {
            let _ = tx.send(Found::Truncated);
            return;
        }
        count += 1;
        let path = entry.path().to_path_buf();
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        let title = if count <= MAX_TITLES { read_title(&path) } else { None };
        let sort = sort_key(&rel);
        if tx.send(Found::Entry(Entry { rel, path, title, sort })).is_err() {
            return;
        }
    }
}

/// Folders that hold no documentation worth listing but can hold huge
/// numbers of files: `node_modules`, cache folders marked with CACHEDIR.TAG
/// (Rust's target/, among others), and ~/Library on macOS.
fn skip_dir(e: &DirEntry, home: Option<&Path>) -> bool {
    if e.depth() == 0 || !e.file_type().is_some_and(|t| t.is_dir()) {
        return false;
    }
    e.file_name() == "node_modules"
        || e.path().join("CACHEDIR.TAG").is_file()
        || (cfg!(target_os = "macos") && home.is_some_and(|h| e.path() == h.join("Library")))
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
        for d in ["docs", ".hidden", "node_modules/pkg", "target/doc"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        let files = [
            ("zeta.md", "# Zeta notes\n"),
            ("README.md", "---\ntitle: x\n---\n\n# Project\n"),
            ("docs/guide.md", "Guide\n=====\n"),
            ("docs/raw.txt", "# not markdown"),
            (".hidden/secret.md", "# hidden"),
            ("node_modules/pkg/README.md", "# a dependency"),
            ("target/CACHEDIR.TAG", "Signature: 8a477f597d28d172789f06886806bc55"),
            ("target/doc/built.md", "# build output"),
        ];
        for (f, body) in files {
            std::fs::write(dir.join(f), body).unwrap();
        }
        let mut p = Picker::start(&dir);
        p.wait();
        assert!(!p.scanning());
        let rels: Vec<&str> = p.entries.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(rels, vec!["README.md", "zeta.md", "docs/guide.md"]);
        let titles: Vec<Option<&str>> = p.entries.iter().map(|e| e.title.as_deref()).collect();
        assert_eq!(titles, vec![Some("Project"), Some("Zeta notes"), Some("Guide")]);

        // Asking for a file before the scan finds it selects it on arrival.
        let mut p = Picker::start(&dir);
        p.select_path(&dir.join("docs/guide.md"));
        p.wait();
        let guide = dir.join("docs/guide.md").canonicalize().unwrap();
        assert_eq!(p.selected_path(), Some(guide));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A picker whose entries are fed by the test instead of a scan.
    fn fed_picker() -> (Picker, Sender<Found>) {
        let (tx, rx) = mpsc::channel();
        let mut p = Picker::start(Path::new("/nonexistent-folio-test"));
        p.wait();
        p.incoming = Some(rx);
        (p, tx)
    }

    fn send(tx: &Sender<Found>, rel: &str) {
        let entry = Entry { rel: rel.into(), path: PathBuf::from(rel), title: None, sort: sort_key(rel) };
        tx.send(Found::Entry(entry)).unwrap();
    }

    fn press(p: &mut Picker, code: KeyCode) -> Action {
        p.key(KeyEvent::new(code, KeyModifiers::NONE), 10)
    }

    fn selected(p: &Picker) -> Option<String> {
        p.selected_path().map(|s| s.to_string_lossy().into_owned())
    }

    #[test]
    fn untouched_selection_stays_at_the_top() {
        let (mut p, tx) = fed_picker();
        send(&tx, "b.md");
        p.pull();
        send(&tx, "a.md");
        p.pull();
        assert_eq!(selected(&p).as_deref(), Some("a.md"));
    }

    #[test]
    fn selection_follows_the_file_the_reader_moved_to() {
        let (mut p, tx) = fed_picker();
        send(&tx, "b.md");
        send(&tx, "c.md");
        p.pull();
        press(&mut p, KeyCode::Char('j'));
        send(&tx, "a.md");
        p.pull();
        assert_eq!(selected(&p).as_deref(), Some("c.md"));
    }

    #[test]
    fn returning_reader_keeps_their_file_even_at_the_top() {
        // The file arrives first (at index 0), then a file that sorts earlier.
        let (mut p, tx) = fed_picker();
        p.select_path(Path::new("zz/guide.md"));
        send(&tx, "zz/guide.md");
        p.pull();
        send(&tx, "README.md");
        p.pull();
        assert_eq!(selected(&p).as_deref(), Some("zz/guide.md"));
        // Opening help doesn't let go of it either.
        assert!(matches!(press(&mut p, KeyCode::Char('?')), Action::Help));
        send(&tx, "AAA.md");
        drop(tx);
        p.pull();
        assert_eq!(selected(&p).as_deref(), Some("zz/guide.md"));
        assert!(!p.scanning());
    }

    #[test]
    fn closing_the_filter_keeps_the_chosen_file() {
        let (mut p, tx) = fed_picker();
        for f in ["a.md", "b.md", "guide.md", "z.md"] {
            send(&tx, f);
        }
        drop(tx);
        p.pull();
        press(&mut p, KeyCode::Char('/'));
        for c in "gui".chars() {
            press(&mut p, KeyCode::Char(c));
        }
        assert_eq!(selected(&p).as_deref(), Some("guide.md"));
        press(&mut p, KeyCode::Esc);
        assert_eq!(selected(&p).as_deref(), Some("guide.md"));
    }

    #[test]
    fn fuzzy_matches_subsequences() {
        assert!(fuzzy_match("dg", "docs/guide.md"));
        assert!(fuzzy_match("inst lin", "Install on Linux"));
        assert!(!fuzzy_match("xyz", "Install"));
    }
}
